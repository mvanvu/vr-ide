# FR-11 Phase 5 minors
- started; base 440b64f
- 1 read-only EDIT: test mode_tests::read_only_file_stays_in_read_mode_on_ctrl_i_tab_and_palette red ("ctrl+i" assertion), green after App::enter_edit_mode refuses + read_only_warning.
- 2 Esc hint: test notice_esc_hint_is_only_shown_when_esc_dismisses_it red (left "careful  (Esc)", right "careful"), green after status_bar::esc_dismisses. Decision logged D? (FR-11).
- 3 wording: "Read mode: ctrl+i to edit", "<file>: read-only (not valid UTF-8)"; README.md, docs/keymap.md, mode_tests HINT updated.
- 4 panics: test recovery::tests::worker_panic_while_the_tui_is_active_is_kept_not_printed (red: did not compile, API absent), green; RecoveryState keeps messages, event_loop::exit / main-thread hook print them after restore. WorkerFailed notice path untouched.
- 5 preview.rs: 40x12 added, Combined scene, title, ctrl+i in Confirm dialog (also dropped the refused 'y'); previews in mockups/FR-11/ (synthwave, dracula, compare).
- Snapshots: no .snap changed (all existing pass unchanged).
- cargo test -p vr-ide: 273 passed, 0 failed, 4 ignored. Guard: fmt + clippy clean.

## Rule compliance
BASE-1 pass, BASE-2 pass, BASE-3 pass (red lines above), BASE-4 pass, BASE-6 pass, BASE-10 pass, BASE-11 pass, BASE-24 pass, BASE-25 pass, BASE-32 pass, BASE-38 pass. n-a: BASE-5, 8, 14, 15, 16, 17, 18, 19(done).

## New states
| State | Set | Cleared | Bound |
|---|---|---|---|
| `RecoveryState.tui_active` (AtomicBool) | `main.rs` after terminal setup succeeds | `event_loop::exit` right after `restore_terminal()`; first line of the main-thread panic hook branch | one flag |
| `RecoveryState.deferred` (Mutex<Vec<String>>) | non-main panic while `tui_active` | `take_deferred()` at exit (after `finish()`) or in the main-thread panic hook | at most `MAX_DEFERRED` = 20 messages |
| `RecoveryState.dropped` (AtomicUsize) | each message past the bound | reset by `take_deferred()`, which appends "and N more worker panics" | counter only |

## Round 1
- M1 race: red = `cargo test -p vr-ide recovery::` failed (4 passed, 2 failed) on 3 of 3 runs before the fix. Fix: `recovery::tests::HOOK_TEST` mutex held for the whole body of every hook-replacing test (incl. `event_loop::tests::edits_are_published_before_a_frame_is_drawn`), per-test counters (`MAIN_ONLY_RESTORED`, `PREVIOUS_CALLS`). Green: `cargo test -p vr-ide recovery::` 20 runs in a row, 20/20 "8 passed".
- m1: deferred prints moved after `finish()` in `event_loop::exit`.
- m2: test `worker_panic_while_main_unwinds_is_printed_not_kept` red (late panic lost), green after `set_tui_active(false)` first in the main branch.
- m3: test `deferred_panics_are_bounded_and_the_rest_counted` red (25 kept, expected 21), green.
- m4: test `read_only_warning_names_the_real_encoding` red ("not valid UTF-8" vs "UTF-16LE, not UTF-8"), green; shared helper `not_utf8_reason` with the open warning.
- `cargo test -p vr-ide`: 276 passed, 0 failed, 4 ignored.
- Guard (`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`) on final code, exit 0:
```
    Checking vr-ide v0.1.0 (/home/rainy/DEV/vr-ide/crates/vr-ide)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.18s
```
