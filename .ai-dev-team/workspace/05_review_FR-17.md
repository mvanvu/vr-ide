# Review FR-17 (light tier, fe)

Tests re-run by reviewer: `cargo test -p vr-ide terminal_right` 14 passed; `cargo test -p vr-config --test config` 23 passed.

## Findings

[major] crates/vr-config/defaults/keymap.toml:123 — Reverses a closed user decision (D279 item 5 / D278) with no new decision (BASE-32)
Scenario: D278 asked "add ctrl+f7/f8/f9 to the [terminal] table so they work inside the terminal?" and the user answered (D279-5) "ctrl+f7/f8/f9 stay editor-only (leave the terminal first)". The fix does exactly the rejected option. AC-F1 itself says "from the editor ... D227 otherwise unchanged". The developer's own root cause shows ctrl+f7 already worked from the editor over VTE bytes, so AC-F1 as written had nothing to fix. If the user's complaint was in fact the focused-terminal case, that is a new user answer superseding D279-5 and must be recorded as a D entry by the leader. No D entry exists, and the dev notes say "no new decision needed".
Fix: leader records the supersession (user confirmation, or confirm-on-return like D313) as a D entry. Otherwise revert the keymap line, the test change and the docs, and close FR-17 as "works from the editor; terminal focus is editor-only per D279-5". The mechanism is sound if approved.

[major] README.md:306-308 and docs/keymap.md:244 — Docs still contradict the change
Scenario: README says "vr-ide keeps only ctrl+` and F1". The change makes three keys, so README is false. The new text at keymap.md:114 and :214 says three. keymap.md:244 ("a focused terminal gets every key except the ones in [terminal]") is generic and fine. README:313 says f8/f9 are "(from the editor)", which is consistent but leaves f7 as the odd one.
Fix: README:307 -> "keeps only ctrl+`, F1 and ctrl+f7 (panel position)". Also docs/commands.md:33 is fine as is.

[minor] README.md:319 and docs/keymap.md:115 — Over-long lines break the file's wrapping (about 100 columns). Rewrap.

[minor] Out of scope, noted by dev: ctrl+f8/f9, f8/shift+f8 still go to the program, so the terminal table is now inconsistent (f7 position works but width grow/shrink does not). Raise it in the same user decision.

## Judgement on program conflict
ctrl+f7 is not used by mc (F7 = mkdir; ctrl+f7 unused), htop, vim, tmux or lazygit by default. Taking it from the program is acceptable technically. The objection is only the D279-5 decision above.

## Checked OK
- The old test asserted "went to the program" on a panel already at Right, so it was stale and could never fail. The new test starts at Bottom, covers kitty and legacy, and is meaningful (14 passed).
- The config test allowlist change is correct and minimal (the entry is exactly ctrl+f7 -> terminal.toggle_position in Terminal context). 23 passed.
- The global ctrl+f7 binding is kept, and the unused imports are removed.

## Files read
| File | Status |
|---|---|
| README.md | read |
| crates/vr-config/defaults/keymap.toml | read |
| crates/vr-config/tests/config.rs | read |
| crates/vr-ide/src/terminal_right_tests.rs | read |
| docs/keymap.md | read |

## Rule check FR-17
| Rule | Result | Note |
|---|---|---|
| BASE-1 | pass | Diff is within the task's scope |
| BASE-2 | n-o | |
| BASE-3 | pass | Red output is in the notes (focused terminal, enhanced=true, left Bottom right Right). Test now passes. |
| BASE-6 | n-o | |
| BASE-25 | pass | fmt and clippy clean per notes. Tests pass here. |
| BASE-32 | fail | The fix supersedes user answer D279-5 with no D entry. |
| BASE-35 | pass | USER-4 and USER-5 are not violated, and the key is unchanged |
| BASE-37 | pass | walk.py before/after is in the notes. It covers the focused-terminal case only. |
| AC-F1 | pass with caveat | The editor case worked already. The focused-terminal case needs the D entry. |
| USER-5 | pass | ctrl-only key that reaches vr-ide in legacy mode |

CHANGES_REQUESTED

## Re-check round 1
Scope: `git diff master -- README.md docs/keymap.md` plus D314.
- BASE-32 finding: fixed. D314 supersedes D279-5 for ctrl+f7 only and is recorded in decisions.md. f8/f9 stay editor-only, which matches the code.
- README finding: fixed. README now lists ctrl+` , F1 and ctrl+f7 and says the other panel keys work from the editor. The two docs/keymap.md spots stay consistent.
- Long lines: fixed. README and docs/keymap.md prose is rewrapped. The long table row in keymap.md is the file's existing style.
- Minor f8/f9 inconsistency: held for the user under D314, accepted.
- No new issues in the docs diff.

| Rule | Result | Note |
|---|---|---|
| BASE-32 | pass | Judged against D314 |
| BASE-1 | pass | Docs-only fix round |
| AC-F1 | pass | Editor and focused-terminal cases are both covered, and D314 is recorded |
| Other rows | unchanged from round 0 | |

APPROVE
