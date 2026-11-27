# Review FR-25: SSH sessions from the command window, full screen after login (D317)

Scope: worktree `.worktrees/FR-25`, task commit 6b4cf8f plus the merge fb6a5de (FR-26, FR-28). I checked where they meet: `render_grid` now fills its whole area with `terminal_bg` (FR-26), and `ssh_view::render` calls `render_grid`, so full screen gets the terminal background. qc's L-K01 confirms this: full screen bg = panel bg, which differs from the editor bg. `ui/mod.rs`: the full-screen branch draws the palette, including the FR-28 detail area, over the session. The palette only uses `area`, not `app.layout`.
Mode: code + security + design QA. Tests run once: `cargo test -p vr-term --test marker` gave 5 passed. `cargo test -p vr-ide --lib ssh` gave 49 passed.

## Findings

[minor] crates/vr-ide/src/ssh/config.rs:76-79 (`tokens`): a trailing comment on a `Host` line becomes extra aliases
Scenario: `Host prod # main server` lists `prod`, `main` and `server`. OpenSSH 8.7 and later ends the argument list at a `#` token. Picking `main` runs ssh to a host that does not exist. Nothing is injected, because each token still passes `Target::parse`.
Fix: in `tokens`, stop at the first unquoted token that starts with `#`. Add a test line in `sec_s11_only_concrete_valid_host_tokens_are_listed`.

[minor] crates/vr-ide/src/commands/ssh.rs:426-435 (`open`): a late probe answer takes the keys
Scenario: a slow `Match exec` or a probe timeout takes up to 5 s. If the user goes back to the editor and types in that time, `reveal_panel` gives the new tab the focus. The next keystrokes go to ssh's host-key or password prompt, so editor text can reach the remote as a password attempt.
Fix: at `start`, record the focus and whether the palette was open. In `open`, take the focus only when both are unchanged. Otherwise push the tab without focus and show a notice ("SSH x: waiting in its tab").

[minor / low] crates/vr-term/src/types.rs:178 (`SpawnExtras` derives `Debug`): this prints the marker, which contains the nonce
`Nonce` hides itself in `Debug` (SEC-S7), but `SpawnExtras { marker: Some(..) }` prints the nonce through the derived `Debug`. Nothing formats it today, so the defect is latent.
Fix: write a manual `Debug` that prints `marker: Some(..)`.

[minor] BASE-16, crates/vr-ide/src/ssh/mod.rs:230 (`find_in_path`) and :259 (`probe`): copies of `vr-git` `find_git` and its spawn/timeout/group-kill pattern
The SEC-S2 PATH rule now lives in two places that can drift. The developer gave a reason (the vr-git API is private). It is acceptable for now; extracting a shared helper is follow-up work.

[low] crates/vr-ide/src/ssh/mod.rs:282-297: the probe's reader thread can block forever
Scenario: a `Match exec` command that runs `setsid` and keeps stdout open is not in the killed group. The pipe never reaches EOF, so one `vr-ssh-probe-read` thread stays blocked per such probe. This leaks a thread, not a process. The UI is unaffected because of `PROBE_GRACE`.
Fix: optional. Give the child a pipe that the parent closes after the deadline, or accept and document it.

Not filed:
- I checked `terminal_pass_nul = true` in a legacy terminal for a lockout in full screen. There is none: the ssh layout has an editor height of 0, so `editor_covered` is true, NUL still toggles, and the status row hint is correct.
- qc's walk (`qa-artifacts/FR-25/qc_walk_result.txt`) shows FAILs at L-S18, L-B01 and L-B02. They come from the walk itself, not from FR-25. The shim ends with `exec /bin/sh`, so `kill -9 $PPID` in the fake remote kills vr-ide. The screen freezes in full screen and the later hostile-target steps act on a dead app. qc should send SIGKILL to the shim's pid.

## Files read
| File | Status |
|---|---|
| README.md | read |
| crates/vr-config/defaults/settings.toml | read |
| crates/vr-config/src/settings.rs | read |
| crates/vr-config/tests/config.rs | read |
| crates/vr-ide/Cargo.toml | read |
| crates/vr-ide/src/app.rs | read |
| crates/vr-ide/src/commands/mod.rs | read |
| crates/vr-ide/src/commands/palette.rs | read |
| crates/vr-ide/src/commands/registry.rs | read |
| crates/vr-ide/src/commands/slash.rs | read |
| crates/vr-ide/src/commands/ssh.rs | read |
| crates/vr-ide/src/commands/terminal.rs | read (diff + focus, quit, close, mouse, NUL callers) |
| crates/vr-ide/src/lib.rs | read |
| crates/vr-ide/src/recovery.rs | read (+ create_private_dir/file bodies) |
| crates/vr-ide/src/slash_tests.rs | read |
| crates/vr-ide/src/snapshots/vr_ide__ssh_tests__ssh_full_screen_60x8.snap | read |
| crates/vr-ide/src/ssh/config.rs | read |
| crates/vr-ide/src/ssh/mod.rs | read |
| crates/vr-ide/src/ssh/recent.rs | read |
| crates/vr-ide/src/ssh/tests.rs | read: test list; SEC-S1, S3, S11 bodies in full; the others by name |
| crates/vr-ide/src/ssh_tests.rs | read: test list; S12, S14, S17 bodies in full; fixtures skimmed |
| crates/vr-ide/src/ui/mod.rs | read (also the merged result) |
| crates/vr-ide/src/ui/ssh_view.rs | read |
| crates/vr-ide/src/ui/terminal_view.rs | read (merged `render`, `render_grid`, `theme_cell_style`) |
| crates/vr-term/src/lib.rs | read |
| crates/vr-term/src/marker.rs | read: matcher in full; unit tests by name |
| crates/vr-term/src/terminal.rs | read (diff + `mark_dirty`, `wait_loop`) |
| crates/vr-term/src/types.rs | read |
| crates/vr-term/tests/marker.rs | read |
| docs/commands.md | read |
| docs/keymap.md | read |

## SEC check
| SEC | Result | Evidence |
|---|---|---|
| S1 | pass | `Target::parse` mod.rs:61-123 is the only constructor; it is used for typed text (`connect`), config (`config.rs:97`) and recent lines (`recent.rs:263`). The allowed-byte rules exclude every listed character. Covered by `sec_s1_refused_targets`. |
| S2 | pass | `argv` mod.rs:207-216 builds `[-l][-p] -o ControlMaster=no [pair] -- host`. `spawn_program` spawns with an argv and no shell. `find_in_path` skips relative and empty PATH entries. `sec_s2_*` tests cover this. |
| S3 | pass | Constant `printf '\033]7717;vr-ide-ssh;<32 hex>\007'` (mod.rs:193). The nonce comes from `rustix::rand::getrandom` (16 bytes, retried on EINTR). Tested with sh, bash and dash, plus a real `ssh -G` echo. zsh and fish are not installed (n-o for those two). |
| S4 | pass | `probe`: stdin null, `setsid`, 5 s, 256 KiB cap, group killed on timeout or cap. A user `localcommand`, a failure or a timeout means no pair is passed. The palette never runs `-G` (`prepare` only reads the config). |
| S5 | pass (argv); sshd part n-o | `ControlMaster=no` is always in the argv. The localhost-sshd mux check needs an sshd, which is not installed. |
| S6 | pass | `MarkerScan` (KMP; holds at most len-1 bytes; removes every occurrence). It runs before `parser.advance` in `read_loop`; flush happens at EOF. `marker::tests` and `tests/marker.rs` cover this. |
| S7 | pass (one latent leak) | The nonce is not in env, notices, labels, the recent file or logs. `Nonce` `Debug` is redacted. `SpawnExtras` `Debug` is not (minor finding). |
| S8 | pass | `marker_seen` belongs to each `Terminal` and only counts while `status == Running`. A reconnect makes a new `Terminal` and a new marker. `sec_s8_*` tests cover this. |
| S9 | pass | The notice is the fixed `NOT_VALID`. The only `tracing` call in the ssh path is the recent-file I/O error. The palette keeps no history. |
| S10 | pass | 0600 `create_new` temp file, `rename`, `symlink_metadata`, 64 KiB, re-validation, at most 20 entries. The file is written only after a login or exit 0. Folder mode: D325 accepts it (an existing folder keeps 775). |
| S11 | pass | `read_regular` checks the metadata before and after a NONBLOCK open. Caps: 1 MiB, 64 files, depth 16, canonical-once, 256 matches and 10 000 entries per glob, 1 000 aliases. `Match` is never evaluated. |
| S12 | pass | `title()` returns `ssh <target>` for sessions and runs `sanitize` on OSC titles of every tab. The pty is the window minus the status row (`fit` on the ssh layout). `sec_s12_*` tests cover this. |
| S13 | pass | Same `spawn_program` emulator setup as shells. OSC 52, OSC 8 and the 8000-query test pass. The 10 MiB volume case is covered by the existing vr-term flood test. |
| S14 | pass | The `[terminal]` keymap is matched before `encode_key`. NUL toggles in full screen (`editor_covered`). Covered by `sec_s14_*`. |
| S15 | pass | `OUTER_TERMINAL_ENV` and `VR_IDE_*` are removed, cwd is `$HOME`, and no variable carries the nonce. `sec_s15_*` and `withheld_variables_*` cover this. |
| S16 | pass | `MAX_SESSIONS` counts live ssh tabs; only one `pending` probe at a time; the panel's scrollback is used. |
| S17 | pass | `shell_in_foreground` is false for ssh tabs, so close asks first and `running()` counts them for quit. A dialog suspends full screen. `shutdown` reaps the group. `ac_s6_sec_s17_*` and `sec_s17_*` cover this. |
| S18 | pass | `sync_full` ends full screen once the tab is not running. There is no timeout fallback. `reconnect` runs probe, nonce and argv again. `ac_s5_*` and `sec_s18_*` cover this. |

## Design QA
I compared the 60x8 snapshot and qc's 120x40 and 80x24 runs with AC-S3 and D317. The status row reads ` SSH  <target>` on the left and the way back on the right. Below about 22 columns the target is shortened first and then the hint is dropped, which is acceptable under the 40-column minimum. The full-screen background equals the terminal panel background (D319/AC-T1). I found no deviations.

## Rule check FR-25
| Rule | Result | Evidence |
|---|---|---|
| BASE-2 | pass | argv, LocalCommand and file format match SEC-S2, S3 and S10 exactly (mod.rs:193-216, recent.rs) |
| BASE-9 | pass | single validator `Target::parse`, used for all three sources |
| BASE-16 | pass (minor finding) | private-file helpers, `sanitize`, `render_grid` and slash rows are reused. `find_in_path` and `probe` copy vr-git (reason given) |
| BASE-32 | pass | D317 (flow), D320 (setting, notice text says why, sanitized titles), D325 (palette entry, full screen only on login with keys, notices deferred, no `-G` when off), D314/D316 (ctrl+`, F1, ctrl+f7..f9 kept: `sec_s14_*`), D315 (scroll code untouched), D319 (terminal bg in full screen through `render_grid`), USER-5 (no new key) |
| BASE-35 | pass | no new key; hints come from `key_hint` |
| BASE-47 | pass | tab bar and status bar both read `TermTab::title()` |
| BASE-6 | pass | English comments that explain why |
| BASE-21 | pass | walk output holds no nonce or password |
| BASE-4 / BASE-3 | pass | notes list red-then-green runs and the final command output |
| BASE-7 | pass | commit 6b4cf8f has no AI trailer; the worktree is clean |
| AC-S1..S6 | pass | see the ssh_tests list; walk shows 38/38 |

APPROVE

## Re-check round 1
Scope: the uncommitted diff in the worktree on top of fb6a5de. I read commands/ssh.rs, app.rs, commands/terminal.rs, ssh/config.rs, ssh/mod.rs, vr-term/src/types.rs, Cargo.toml, the 4 new `ac_s2_a_late_probe_answer_*` tests and the 6 slash snapshot diffs. `cargo test -p vr-ide --lib ssh`: 54 passed.

| Finding | Result | Evidence |
|---|---|---|
| #2 late probe takes the keys (MAJOR after re-grade) | fixed | Details in the path check below. |
| #1 `Host x # comment` | fixed | config.rs:203: an unquoted token that starts with `#` ends the line; `foo#bar` stays one token, as in OpenSSH. Test line added. |
| #3 `SpawnExtras` Debug | fixed | types.rs: a manual `Debug` prints `marker: Some("..")`. vr-term test `debug_output_of_the_spawn_extras_hides_the_marker`. |
| #5 probe reader thread | fixed | `read_until` (mod.rs:302): `poll` waits only for the time left before the deadline; POLLHUP leads to `read` returning 0, which ends it. `checked_duration_since` gives `None` once the deadline has passed. The pipe is moved in and dropped on every return, so the fd is closed. After `None` the group is killed and `child.wait()` reaps. EINTR is retried. No thread is left. Test `sec_s4_a_helper_outside_the_group_*`. |
| #4 BASE-16 | reason accepted | Only the 10-line PATH rule is still shared. Moving it means a new public API in vr-git, which is outside the board row's files. The follow-up `vr_git::find_program` is noted for the leader. |

Path check for #2. `KeysAt` is snapshotted in `start`; the new tab gets the keys only when `KeysAt` is unchanged and neither a palette nor a dialog was open:
- **Session list or palette:** `slash::enter` closes the palette before `run` (slash.rs:809). The snapshot therefore sees `palette=false` and `focus=prev_focus`, and the Enter that submitted was counted before the snapshot. The normal path still takes the keys (`takes_the_keys_when_nothing_happened_meanwhile`).
- **Palette opened, and possibly closed again, while the probe runs:** F1 and Esc each add to `input_count`, so the new tab does not take the keys (`leaves_an_open_command_window_alone`).
- **Dialog opened in the background:** the `dialog` field changes, so the tab does not take the keys.
- **Typing in another tab:** `active` and `input_count` change, and `push_tab(.., false)` keeps the active tab (`does_not_switch_the_tab_the_user_types_in`).
- **No tab existed before:** after `push_tab(false)`, `active` 0 points at the new tab. `show_panel` restores the focus (Editor), so the keys stay in the editor.
- **Reconnect (replace slot):** the focus is not changed. The keys reach the tab only if the user is still on it.
- **`ssh_fullscreen=false`:** `open` runs inside `start` with the same snapshot.

`input_count` counts:
- every key except releases (repeats included), since all keys pass `route_key`
- pastes
- mouse presses

It does not count wheel, move, drag or release events; none of them moves focus or sends text, so that is enough.

Remaining note, not a finding: in full screen the "ready — click its tab" notice is deferred and an info notice can expire before the user leaves full screen. D325 accepts this.

The developer's claim about the snapshots holds. Each of the 6 slash snapshot diffs adds only the FR-28 detail area: a separator line, then the wrapped description of the cut selected row (`/save-as …`, `/theme …`), at most 3 lines, ending in "…". The commit 6b4cf8f did not need these changes (its slash runs were green), and the FR-28 merge ce972e7 updated only `render_palette_40x12`. So master itself carries these 6 stale slash snapshots until FR-25 is merged. That is for the leader, not a defect of FR-25.

SEC rows updated:
- S4: pass. The probe's read is now bounded by `poll` against the deadline; no reader thread.
- S7: pass. The `SpawnExtras` Debug is redacted; the latent leak is gone.
- S11: pass. Trailing `#` comments are honoured.

All other SEC rows are unchanged (pass). The Rule check is unchanged: BASE-16 pass with an accepted reason, BASE-32 pass (D317/D325: full screen only on login while the tab has the keys, a notice otherwise).

APPROVE
