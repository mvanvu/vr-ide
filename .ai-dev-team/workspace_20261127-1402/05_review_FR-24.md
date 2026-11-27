# Review FR-24

Tools: Read, Bash, Write, Skill, SendMessage, SubagentHandback.

No findings. `cargo test -p vr-ide --lib ctrl_f8`: 1 passed (kitty and legacy bytes, Right and Bottom). The test also checks that f8 leaves the active tab unchanged.
Consistency grep (ctrl+f7, "keeps only", "every key"): README:310-317, docs/keymap.md:114-115 and :214 list exactly ctrl+`, F1, ctrl+f7, ctrl+f8, ctrl+f9. No hint, welcome or which-key source lists the terminal keys. docs/commands.md and README:330 only mention ctrl+f8 / ctrl+f9 as width keys, which is still true.

## Files read
| File | Status |
|---|---|
| crates/vr-config/defaults/keymap.toml | read |
| crates/vr-config/tests/config.rs | read |
| crates/vr-ide/src/terminal_right_tests.rs | read |
| README.md | read |
| docs/keymap.md | read |

## Rule check FR-24
| Rule | Result | Note |
|---|---|---|
| BASE-1 | pass | AC-F6 is covered by the test and by the docs. |
| BASE-3 | pass | The test was run in this review. |
| BASE-6 | pass | Notes show fmt and clippy clean; nothing is left behind. |
| BASE-25 | pass | The notes have a rule compliance section and the diff matches it. |
| BASE-32 | pass | D316: f8 / shift+f8 are not bound in [terminal], and the test shows f8 goes to the program. D314 is kept. The D279 allowlist is extended in config.rs. |
| BASE-35 | pass | USER-5: ctrl+f8 / ctrl+f9 are legacy-terminal keys and the test sends legacy bytes. Each command still has one key. |

APPROVE
