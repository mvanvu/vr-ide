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
