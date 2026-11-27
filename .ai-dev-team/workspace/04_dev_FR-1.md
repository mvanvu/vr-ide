# FR-1 Second --session window must not resume saved agents (P5 F-A)

Worktree `.worktrees/FR1`, branch run4-FR1 (from master ca60805). Nothing staged or committed.

## Progress
- read session.rs, ai/restore.rs, session_tests.rs, ai_tests.rs
- red tests added (session_tests, ai_tests), confirmed failing
- fix in session.rs `restore()`; lock-error test added
- related run green; whole crate, fmt, clippy green

## Repro (red)
`cargo test -p vr-ide --lib -- a_second_session_window` before the fix:
```
test session_tests::a_second_session_window_gets_no_ai_section ... FAILED
test ai_tests::a_second_session_window_resumes_no_agent ... FAILED
panicked at crates/vr-ide/src/session_tests.rs:459:5   (take_restored_ai(second) was Some)
panicked at crates/vr-ide/src/ai_tests.rs:555:5        ("nothing resumed twice": second window spawned claude --continue)
test result: FAILED. 1 passed; 2 failed
```

## Root cause
`session::restore` set `keeper.restored_ai = Some(s.ai)` whatever the lock state, and
`ai::restore::from_session` only checks `app.session.is_some()`, so a window without the lock
spawned every saved agent with `spawn::again_mode` in the same folder/account as the lock holder.

## Change
- `crates/vr-ide/src/session.rs`
  - `Restore` carries `other_window` (lock held elsewhere, `Ok(None)`), set in `start()`.
  - `restore()`: `restored_ai` is handed out only when `keeper.saves()`. Otherwise, when the
    saved section has sessions, one warning:
    - another window holds the lock: "AI sessions not reopened: they run in the other vr-ide --session window"
    - lock error: "AI sessions not reopened: this window does not keep the session"
  - module / `take_restored_ai` docs say the section goes only to the saving window.
- `ai/restore.rs` unchanged: `take_restored_ai` now returns `None` in a non-saving window, so
  `from_session` reopens nothing; a second guard there would duplicate the single source of truth.
- The ANOTHER_WINDOW notice text is unchanged (AC/D2); files, tabs, terminals still restore in the second window.

## Tests added
- `session_tests::a_second_session_window_gets_no_ai_section` — second window: no AI section, exact two notices, workspace still restored; first window still gets the section, no notices.
- `session_tests::a_window_that_cannot_lock_gets_no_ai_section` — `session.lock` is a directory (lock error): no AI section, notice `AI_NOT_KEPT`.
- `ai_tests::a_second_session_window_resumes_no_agent` — end to end through `from_session`: keeper resumes claude with `--continue` (AC-8/D7); second window has 0 agents, panel hidden, two notices; an agent opened by hand there works and never reaches the file.

## Commands (green)
```
cargo test -p vr-ide --lib -- session ai_tests restore
test result: ok. 72 passed; 0 failed; 0 ignored; 863 filtered out; finished in 1.66s

$TEAM_BIN/run-heavy.sh cargo test -p vr-ide
test result: ok. 928 passed; 0 failed; 7 ignored   (lib)
test result: ok. 3 passed; 0 failed                 (tests/desktop_cli.rs)

rustfmt --edition 2024 --check crates/vr-ide/src/session.rs crates/vr-ide/src/session_tests.rs crates/vr-ide/src/ai_tests.rs
fmt-ok
$TEAM_BIN/run-heavy.sh cargo clippy -p vr-ide --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 45.41s   (no warnings)
```
Guard per profile map for `crates/vr-ide/**`: `cargo test -p vr-ide` + `cargo clippy -p vr-ide --all-targets -- -D warnings` — both above.

Profile guard exactly as written (BASE-25, leader's gate), in .worktrees/FR1:
```
cargo fmt --all --check && $TEAM_BIN/run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings
FMT-OK
    Checking vr-fs / vr-search / vr-git / vr-term / vr-syntax / vr-core / vr-config v0.1.0 ...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.89s
exit=0
```

## Files changed
- crates/vr-ide/src/session.rs
- crates/vr-ide/src/session_tests.rs
- crates/vr-ide/src/ai_tests.rs

## Deviations
- Notice wording: the brief's "AI sessions run in the other window" became "AI sessions not reopened: they run in the other vr-ide --session window" (matches the existing "Not reopened: …" phrasing); a distinct text for the lock-error case, where no other window is known to exist.

## Out-of-scope issues noticed
- none

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| "does this window save" | `Keeper::saves()` session.rs | reused |
| two-window harness | `session_tests::launch`, `ai_tests::session_window` / `session_dirs` / `wait_for` | reused |
| warning notice | `App::warn` | reused |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-1..4 / D1–D3 | pass | second window shows the unchanged ANOTHER_WINDOW text and never writes (`a_second_session_window_runs_without_saving`, `a_second_session_window_resumes_no_agent`); restore of files/tabs unchanged (whole crate green) |
| AC-8 / D7 | pass | keeper still resumes with `--continue`: `ai_tests::a_second_session_window_resumes_no_agent`, `the_panel_round_trips_through_the_session_file` |
| SEC-6 | pass | file format and write path untouched; diff only gates `restored_ai` |
| BASE red-first | pass | red output above (session_tests.rs:459, ai_tests.rs:555) |
| BASE-11 | pass | fix at root cause (`session::restore`), no symptom patch in the panel |
| BASE-14 | pass | long runs as background tasks, no polling |
| BASE-15 | pass | only own background tasks; none left |
| BASE-16 | pass | Reuse table |
| BASE-19 | pass | progress lines above |
| BASE-24 | pass | nothing staged or committed (`git status`: 3 modified files) |
| BASE-25 | pass | profile guard `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` → FMT-OK, exit=0; plus whole-crate test green |
| BASE-38 | pass | rustfmt run only on the 3 changed files |
| COST related + whole crate once | pass | related run, then one `run-heavy.sh cargo test -p vr-ide` |
| COST-8 | pass | edits via perl -pi / Edit, no python heredocs; throwaway snippets in qa-artifacts/FR-1 |
| COST-9 | pass | nothing left running |
| COST-13 | pass | nothing written to $TEAM_TOOLS |
| scope | pass | ai/changes.rs, ai/review.rs, commands/*, desktop.rs, ui/* untouched |

DONE

## Review — 2026-11-27

Verdict: **APPROVED** (no blocker/major).

### Findings
[minor] crates/vr-ide/src/session.rs:256 — the second window also skips the saved panel width
Scenario: saved `[ai] width = 40`; the second `--session` window opens an agent by hand → the panel opens at the settings default width, not 40. No agent is resumed (correct); only the layout half of the section is dropped with it.
Fix (optional): in the non-saving branch apply `s.ai.width` only (clamped as in `ai::restore::restore`), or leave it as is — the window is not saved, so nothing is lost.

### Checked
- T10 interaction: `Keeper::saves()` = `writer.is_some() || lock.is_some()`; `Keeper::new` moves the lock into the writer, or keeps it when `Writer::start` fails (`Err(lock)`), and `start()` gives `None` on `Ok(None)` / `Err`. So `saves()` is true exactly when this window writes; `tick` and the quit save gate on the same call.
- `ai::restore::from_session` only reads `take_restored_ai`, so `None` there means no spawn; `capture` of the empty panel goes to `set_ai`, which a non-saving keeper never writes.
- ANOTHER_WINDOW text unchanged; workspace roots/tabs still restored (asserted in `a_second_session_window_gets_no_ai_section`); notice only when `s.ai.sessions` is non-empty; lock-error path has its own text and test.
- Red-first output and BASE-25 guard present in notes.

### Files read
- crates/vr-ide/src/session.rs — read
- crates/vr-ide/src/session_tests.rs — read
- crates/vr-ide/src/ai_tests.rs — read

## Rule check FR-1
| Rule | Status | Evidence |
|---|---|---|
| AC-1..4 / D1–D3 | pass | session.rs:252-266 gates only `restored_ai`; ANOTHER_WINDOW unchanged; second window still gets workspace (session_tests new test) and never writes (ai_tests new test: no `gemini` in file) |
| AC-8 / D7 | pass | keeper still resumes with `--base|--continue|` (ai_tests.rs `a_second_session_window_resumes_no_agent`) |
| Second window reopens no agent, one notice only with agents | pass | `else if !s.ai.sessions.is_empty()` → one `app.warn` |
| Lock-error path | pass | `other_window` false on `Err` → AI_NOT_KEPT; `a_window_that_cannot_lock_gets_no_ai_section` |
| T10 `saves()` == will save | pass | see Checked |
| SEC-6 / security | pass | no new input, file format or write path; fewer processes spawned |
| BASE red-first | pass | notes: two FAILED tests at session_tests.rs:459, ai_tests.rs:555 |
| BASE-11 | pass | fix at `session::restore`, single source for `restored_ai` |
| BASE-16 | pass | reuses `Keeper::saves`, `App::warn`, test harnesses |
| BASE-25 | pass | guard command and exit=0 in notes |
