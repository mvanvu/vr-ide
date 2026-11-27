# T1 notes
Files: crates/vr-config/src/settings.rs (fields `ai_auto_switch`, `ai_auto_switch_prompt`, prompt check in `parse`), crates/vr-config/defaults/settings.toml (documented keys after `ai_width_percent`), crates/vr-config/tests/config.rs (`ai_auto_switch_defaults_and_override`).
Red: `cargo test -p vr-config ai_auto_switch` -> error[E0609]: no field `ai_auto_switch_prompt` on type `Settings` (6 errors).
Green: `cargo test -p vr-config` -> 17+27+8 passed, 0 failed; `ai_auto_switch_defaults_and_override ... ok`. clippy -D warnings clean; rustfmt --check clean.
## Reuse
| Need | Reused |
| validation | `parse()` checks + `ConfigWarning::InvalidValue` via per-key `Settings::load` |
## Rule compliance
| Rule | Status |
| AC-1 | pass |
| BASE (all) | pass / n-a (single crate, no commit, nothing started) |
| COST | pass (light direct runs, nothing left running) |
