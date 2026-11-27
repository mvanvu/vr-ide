## QC T1
Engine: cargo (tui project, no browser). Throwaway harness: `$TEAM_WS/qa-artifacts/T1/` (standalone crate, path dependency on vr-config, run with CARGO_TARGET_DIR inside the artifact dir, target removed afterwards; no repo files added).

| Check | Result |
| `cargo test -p vr-config` | 17+27+8 passed, 0 failed; `ai_auto_switch_defaults_and_override ... ok` |
| user file absent / keys absent | switch=true, prompt "Continue where you left off.", 0 warnings |
| override `ai_auto_switch=false`, prompt "Go on." | applied, 0 warnings |
| prompt of 200 chars (ASCII and 200 x "é") | accepted, 0 warnings |
| prompt of 201 chars | default kept, 1 warning (InvalidValue) |
| tab character | default kept, 1 warning |
| multi-line (basic string `\n` and multi-line TOML string) | default kept, 1 warning each |
| empty prompt | default kept, 1 warning |
| `ai_auto_switch = "yes"` | switch=true (default), warning "invalid type: string "yes", expected a boolean" |
| prompt = 5 (wrong type) | default kept, 1 warning |
| bad prompt + valid `ai_auto_switch=false` together | switch=false applied, prompt default, 1 warning (keys isolated) |

## Rule check T1
| Rule | Status | Evidence |
| AC-1 defaults true / "Continue where you left off." | pass | absent-file and absent-keys runs |
| AC-1 user overrides apply | pass | override_ok run |
| AC-1 empty / >200 / control char -> warning + default kept | pass | 201, tab, newline, empty runs |
| Boundary 200 chars accepted (counted in chars) | pass | len200, 200_multibyte |
| Wrong type -> warning + default | pass | switch_yes, prompt_wrongtype |

## QC T2
Engine: Rust harness (no TUI/browser; tui project, driving the TUI judged impractical). Scratch crate `qa-artifacts/T2/qa_t2` calls the public `vr_ide::ai::history::{ensure, shared_dir}` on temp dirs (fake HOME, nothing under the real ~/.claude, ~/.codex or ~/.local/state/vr-ide touched). Built with `run-heavy.sh cargo build --offline` (separate target dir, removed after). Output: `qa-artifacts/T2/run.log`.
Real output lines:
- C1: `Linked`; link == canonical shared path; sha256 a.jsonl d27a9131...8158 before = after (in shared), b.jsonl f2c82dec...51ad before = after; via link lists [a.jsonl, b.jsonl].
- C5 (second run): `Linked unchanged=true`.
- C2 clash: `NotShared("a conversation exists on both sides; left in place")`, all hashes unchanged, entry still a dir, other.jsonl not moved to shared.
- C3 link elsewhere: `NotShared("history entry links elsewhere; left as it is")`, link and target file unchanged, shared empty.
- C4 shared 0755: `NotShared("shared history folder ... is open to others; run chmod 700 ...")`, mode still 755, account entry still a dir with file.
- C6 codex: shared_dir = $CODEX_HOME/sessions; unset -> $HOME/.codex/sessions; claude honours CLAUDE_CONFIG_DIR; ensure -> `Linked`, link = $CODEX_HOME/sessions.
Not covered: real-binary end-to-end (spawn_session wiring is covered by the developer's `named_sessions_share_the_tools_history`, not rerun).

## Rule check T2
| Rule | Result | Evidence |
|---|---|---|
| AC-2 (link, recursive merge, refusals touch nothing) | PASS | C1, C2, C3, C5, C6 |
| SEC-19 (one entry, foreign link kept, no names in reasons) | PASS | C3, fixed reason texts |
| SEC-20 (move not loss, idempotent, clash leaves all) | PASS | C1, C2, C5 hashes |
| SEC-21 (shared folder 0755 refused, mode unchanged, created 0700, env-derived path) | PASS | C4, C6 |
