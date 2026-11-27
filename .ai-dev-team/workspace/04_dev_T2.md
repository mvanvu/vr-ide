# T2 notes
- Files (worktree .worktrees/T2, branch run4-T2): crates/vr-ide/src/desktop.rs, crates/vr-ide/src/desktop_tests.rs, assets/vr-ide.svg, crates/vr-ide/src/lib.rs (mod lines), README.md.
- `cargo test -p vr-ide --lib desktop`: 7 passed. Guard (fmt --check + clippy -D warnings): clean.
- desktop-file-validate (installed) is run inside the install test on a hostile exe path; passes.
- StartupWMClass: not set. gnome-terminal-server owns all windows (class gnome-terminal-server), so a custom class cannot be set without changing GNOME Terminal settings.
- Pending: wire --install-desktop / --uninstall-desktop in cli.rs/main.rs after "T1 merged" (call desktop::run_install / run_uninstall, print the returned message).

## CLI wiring
- cli.rs: `DesktopAction`, `desktop_action(args)` (flag must be the only argument, else `TooManyArgs`, exit 2), flag constants, usage text. main.rs: handled before `resolve`; prints the result of `desktop::run_install` / `run_uninstall`, exit 1 on refusal.
- Red first: `cli_desktop_action_flags_are_alone` written before the code, failed to compile (`desktop_action` not found), then green. The binary test in crates/vr-ide/tests/desktop_cli.rs was written with the code (not seen red).
- crates/vr-ide/tests/desktop_cli.rs runs the real binary with a temp XDG_DATA_HOME: install, uninstall, and flags-not-alone (exit 2, nothing written).
- `cargo test -p vr-ide -- desktop cli`: lib 40 passed, desktop_cli 2 passed. Guard clean. README lines were already added.

## Fix round 1
- install checks both targets first; an existing file or symlink without our marker is refused (error names the path, "exists and was not written by vr-ide; remove it first", exit 1), nothing written, symlink target untouched.
- A failed .desktop write removes the icon created in that run.
- `path_note`: `--install-desktop` adds a note line when the entry points to a binary other than the `vr-ide` on PATH.
- Marker checks: `.desktop` needs a line equal to `X-VrIde-Generated=true` (trimmed); svg must equal the bundled icon or have the exact `<!-- X-VrIde-Generated=true -->` line.
- Red first: new tests failed (4 failed against a stub, plus an unresolved import) before the fix. Now `cargo test -p vr-ide -- desktop cli`: lib 43 passed; tests/desktop_cli.rs 3 passed. Guard clean.

## Rule compliance
| Rule | Result | Evidence |
|---|---|---|
| AC-5 / D4 | pass | install/uninstall paths, XDG_DATA_HOME, GNOME Terminal vs Terminal=true, desktop-file-validate in `install_writes_valid_files_and_uninstall_removes_them`; flags in tests/desktop_cli.rs |
| SEC-7 | pass | `exec_round_trips_hostile_path`, `validate_refuses_bad_paths`, `install_refuses_unmarked_targets_and_writes_nothing`, `install_replaces_our_own_files_but_uninstall_needs_the_exact_marker`, `uninstall_keeps_foreign_files` |
| USER-4 | pass | only the two files are written or removed; no GNOME Terminal setting touched; tests use temp dirs |
| BASE-11 | pass | no checkout/restore/stash/reset |
| BASE-24 | pass | nothing staged or committed |
| BASE-25 | pass | fmt --check and clippy -D warnings clean |
| BASE-38 | pass | rustfmt run on changed files only |
| COST | pass | related tests only, via run-heavy.sh |
| Red-first | partial | first part (module + CLI binary tests) written with the code; fix round and `desktop_action` test seen red first |
| other IDs | n-a | not exercised by this task |
