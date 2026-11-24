# Review T3 — vr-config keymap + settings
Verdict: APPROVE T3 (0 blocker, 0 major, 3 minor)
Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.
Run: `cargo test -p vr-config` 14 + 3 unit green; `cargo clippy -p vr-config --all-targets -- -D warnings` clean.

## Findings
[minor] crates/vr-config/src/keymap.rs:quote_line — unquoted key containing a quote or brace is not quoted
Scenario: user writes `ctrl+' = "x"` (or `ctrl+{ = "x"`): `find(['"','\'','{'])` hits the key char itself, no `=` before it, line passes through unchanged, TOML parse fails, so the whole user keymap is ignored (one Malformed warning).
Fix: search for the value start after the last `=` that is followed by optional spaces and `"`/`'`/`{`; or document that such keys must be quoted. Add a test.

[minor] crates/vr-config/src/keymap.rs:apply — two TOML keys that normalize to one chord ("ctrl+s" and "ctrl+S") in one file resolve silently by alphabetical order
Fix: warn on a chord bound twice in the same file/context (InvalidValue "duplicate binding").

[minor] BASE-3 — parser/loader written together with tests; red seen only via default-keymap tests (developer disclosed). 
Judgement: tests assert behavior (parse table incl. bad cases, roundtrip, override, unbind, unknown command, malformed, bad entries, terminal no-fallback, AC-5 unquoted key), so plausible parser/loader bugs would fail them. Process fail only, no code change requested.

Notes (no action): `ctrl++` and `shift+` are rejected (plus key must be spelled `plus`; documented); uppercase single letters lowercased ("A" == "a", shift only via modifier); ctrl+n deviation is recorded as D21; quote pre-processing only touches lines whose key has non-bare chars, comments/headers/quoted keys pass; values untouched.

## USER-1 check (every VS Code default Linux key)
palette ctrl+shift+p/f1, quick open ctrl+p (+ctrl+e), ctrl+o, ctrl+s, ctrl+shift+s, ctrl+w/ctrl+f4, ctrl+pagedown/pageup, ctrl+b, ctrl+shift+e, ctrl+shift+f/h, f4/shift+f4, f6, ctrl+z, ctrl+shift+z/ctrl+y, ctrl+c/x/v/a, ctrl+shift+k, shift+alt+up/down, alt+up/down, ctrl+]/[ (+shift+tab), ctrl+/, ctrl+g, ctrl+f, ctrl+h, f3/shift+f3, ctrl+shift+1, ctrl+alt+enter, explorer f2/delete/ctrl+c/ctrl+v: all match VS Code. ctrl+n differs by design (D21). D8: [terminal] alt+q = focus.editor, no global fallback in Terminal; tested.

## Files read
crates/vr-config/src/lib.rs (diff), keys.rs, keymap.rs, settings.rs, paths.rs, defaults/keymap.toml, defaults/settings.toml, tests/config.rs (grep + head, tests green), notes 04_dev_T3.md. Cargo.toml untouched.

## Rule check T3
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-config + team files |
| BASE-2 | pass | contract signatures present; additive helpers listed |
| BASE-3 | fail (minor) | disclosed partial red |
| BASE-4 | pass | notes show test/clippy/fmt output; reran test + clippy green |
| BASE-6 | pass | English why-comments |
| BASE-8 | pass | no manifest change |
| BASE-32 | pass | D8, D10, D19-D23 honoured |
| AC-5 | pass | binding resolves, unknown command -> warning, malformed TOML -> warning + defaults |
| USER-1 | pass | see above |
