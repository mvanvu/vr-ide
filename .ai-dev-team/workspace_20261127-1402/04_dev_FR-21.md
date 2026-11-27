# FR-21 Explorer colours: folders and file extension groups from theme tokens
- Skills loaded: ai-dev-team:implementation, ai-dev-team:ui-ux-design (contrast / authentic design only)
- Tools available: Agent, Artifact, Bash, Edit, Read, Skill, ToolSearch, Write, SubagentHandback (no sub-agents used)
- Worktree: /home/vu-dev/DEV/vr-ide/.worktrees/FR-21 (branch fr-21)
## Progress
- read explorer.rs, theme.rs, synthwave.rs; no explorer insta snapshots exist in the repo, no git markers in explorer rows
- code: theme/explorer.rs (ExplorerKind, tables), tokens in dark/light/dracula/synthwave, kind_of + render in ui/explorer.rs, README line
- tests green; guard (fmt + clippy -D warnings) clean; temp preview test removed after generating artifacts

## Files changed
crates/vr-ide/src/theme.rs, crates/vr-ide/src/theme/explorer.rs (new), crates/vr-ide/src/theme/synthwave.rs, crates/vr-ide/src/ui/explorer.rs, crates/vr-ide/src/explorer_tests.rs, README.md

## Tests added
ui::explorer::tests::kinds_by_extension_name_and_case; theme::tests::explorer_kinds_read_on_the_explorer_and_selection_backgrounds (both depths, 4.5 on bg, 3.0 on selection); theme::tests::explorer_kinds_are_told_apart_in_every_theme; explorer_tests::explorer_names_take_their_kind_colour (drawn buffer).

## Commands
- cargo test -p vr-ide --lib -- explorer kinds_ : 72 passed
- cargo test -p vr-ide --lib -- ui:: theme:: explorer : 157 passed, 2 ignored; vr-config --test config: 23 passed
- cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings: clean
- Snapshots: no explorer colour snapshots exist (existing explorer text snapshots unchanged, none regenerated); colour is asserted on the drawn buffer instead.

## Design
Group -> token (Theme::explorer(kind)): Folder, CodeNative (rs c cpp go java kt swift ...), CodeScript (js ts vue php py sh html css ...), Config (toml json yaml xml, Makefile, Dockerfile, .gitignore), Docs (md txt pdf, README/LICENSE), Media (images audio video fonts), Archive (archives, exe/so/wasm), Generated (*.lock, package-lock.json, go.sum, .min.js, .map), Hidden (other dotfiles, italic), Plain (default text).
The focused cursor row keeps list_selected foreground (no kind colour); unfocused cursor / multi-selected rows keep the kind colour on the selection background (>= 3:1); ignored / missing / broken rows still get the dim patch. The explorer has no git markers.
dark/light use ANSI colours (terminal-defined, Reset background), so ratios cannot be computed; they are checked for distinctness only.

Contrast (true colour; indexed-256 also verified by test):
| theme | kind | rgb | contrast on bg | on selection |
|---|---|---|---|---|
| dracula | Folder | #bd93f9 | 5.90 | 3.79 |
| dracula | CodeNative | #ff79c6 | 5.97 | 3.84 |
| dracula | CodeScript | #50fa7b | 10.38 | 6.67 |
| dracula | Config | #ffb86c | 8.36 | 5.37 |
| dracula | Docs | #f1fa8c | 12.74 | 8.19 |
| dracula | Media | #8be9fd | 10.29 | 6.61 |
| dracula | Archive | #ff7878 | 5.57 | 3.58 |
| dracula | Generated | #8d9bc8 | 5.19 | 3.34 |
| dracula | Hidden | #9aa8d4 | 6.05 | 3.89 |
| dracula | Plain | #f8f8f2 | 13.36 | 8.59 |
| synthwave | Folder | #c77dff | 6.80 | 4.43 |
| synthwave | CodeNative | #ff7edb | 8.06 | 5.25 |
| synthwave | CodeScript | #72f1b8 | 13.05 | 8.50 |
| synthwave | Config | #ff9d3c | 8.83 | 5.75 |
| synthwave | Docs | #ffe66d | 14.62 | 9.52 |
| synthwave | Media | #82aaff | 7.96 | 5.18 |
| synthwave | Archive | #ff6e7f | 6.78 | 4.42 |
| synthwave | Generated | #9d8bd0 | 6.13 | 3.99 |
| synthwave | Hidden | #b9a6e6 | 8.41 | 5.48 |
| synthwave | Plain | #f6eefc | 16.15 | 10.52 |

Preview (USER-2): qa-artifacts/FR-21/explorer-themes.ans (dracula and synthwave, 34x24 TestBackend, view with cat); table at qa-artifacts/FR-21/contrast.md.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| contrast fitting per theme | capabilities.md theme section, theme.rs | reused fit_floors, syntax-style tables and ratio test helper |
| theme token plumbing | theme.rs | extended Theme with explorer_kinds (same pattern as captures) |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-F5 | pass | `theme::tests::explorer_kinds_read_on_the_explorer_and_selection_backgrounds` (dracula + synthwave, true colour and 256: >= 4.5:1 on bg, >= 3:1 on selection); table in `qa-artifacts/FR-21/contrast.md`; `explorer_tests::explorer_names_take_their_kind_colour`; `ui::explorer::tests::kinds_by_extension_name_and_case`; colours read via `Theme::explorer`, no hard-coded colours in `ui/explorer.rs` |
| AC-F5 (dark / light) | pass | ratios of terminal-defined colours depend on the user palette, so `explorer_kinds_of_dark_and_light_read_on_common_terminal_backgrounds` checks every kind at >= 4.5:1 with standard xterm RGB against white (light) and #300a24 and #000000 (dark); distinctness in `explorer_kinds_are_told_apart_in_every_theme` and `explorer_generated_and_hidden_differ_by_colour_in_every_theme` |
| USER-2 | pass | terminal-viewable preview `qa-artifacts/FR-21/explorer-themes.ans` (TestBackend, dracula + synthwave) |
| USER-3 | pass | built directly, no mockup round |
| BASE-1 | pass | diff limited to theme, explorer, its tests, README |
| BASE-3 | pass | unit, contrast and render tests added for each AC-F5 behaviour and run green (written together with the code, not seen red first) |
| BASE-4 | pass | trimmed test and guard output in Commands above |
| BASE-5, COST-1 | pass | tests and clippy ran through `run-heavy.sh` |
| BASE-10, COST-10 | pass | related set: `explorer`, `ui::`, `theme::`, `vr-config --test config` (files named by `rule-checks.sh related-tests`) |
| BASE-14, BASE-15, COST-3, COST-5, COST-9 | pass | no polling, nothing backgrounded, nothing left running |
| BASE-16 | pass | `## Reuse` table: reused `fit_floors` and the table pattern of `captures` |
| BASE-18, BASE-19 | pass | no sub-agents; notes created at start with progress lines |
| BASE-6, BASE-7, BASE-22 | pass | English comments, no commit, names follow `theme/` neighbours |
| BASE-11, BASE-12, BASE-24 | pass | worked only in worktree `.worktrees/FR-21`; no stash/checkout/reset, nothing staged |
| BASE-25 | pass | `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` clean |
| BASE-32 | pass | D311 kept: colours only, no icons, theme tokens, 4.5:1 |
| BASE-35 | pass | USER-2 and USER-3 above |
| BASE-38 | pass | `rustfmt` run on the changed files only |
| BASE-45 | pass | AC-F5 text matches `01_spec_FR17-21.md` |
| BASE-46 | pass | tests and preview ran on the final code |
| COST-2, COST-4, COST-6, COST-7 | pass | only own related tests, no full suite, no audits, no browser |
| COST-8 | pass | batched reads and edits, no python heredocs |
| COST-13 | pass | throwaway preview output under `qa-artifacts/FR-21/`; temporary test removed |
| COST-15, COST-16 | pass | no snapshots needed or regenerated; outputs trimmed |

n-a: BASE-2 (no contracts) · BASE-8 (no dependency) · BASE-9, BASE-21, BASE-41 (no input, secrets or logins) · BASE-13, BASE-20 (nothing deleted) · BASE-17, BASE-28 (no tool or third-party code touched) · BASE-23 (one render path) · BASE-26 (no rewrite) · BASE-27, BASE-30, BASE-31 (no shared data, containers or checks that create records) · BASE-29, BASE-33, BASE-34, BASE-40, BASE-43, BASE-44, BASE-48 (no browser, other system, blocked step or fake data) · BASE-36 (single app) · BASE-37 (not a runtime-defect fix) · BASE-39 (no schema) · BASE-42 (no test settings changed) · BASE-47 (no computed value fixed)
n-a: COST-11 (no e2e) · COST-12 (leader) · COST-14 (no CI or deploy wait) · COST-17 (no golden merge) · COST-18 (no full run) · COST-19 (no qc measurements) · COST-20 (not Flutter)
n-a: USER-35 (no such line in `$TEAM_STATE/user-rules.md`; USER-2 and USER-3 are the applicable ones)

## Fix round 1 (review 05_review_FR-21.md)
- Red first: new tests `explorer_kinds_of_dark_and_light_read_on_common_terminal_backgrounds` (failed: light Docs Indexed(30) 4.36) and `explorer_generated_and_hidden_differ_by_colour_in_every_theme` (failed) seen failing before the colour change.
- fixed: light Hidden Gray -> Indexed(243) italic; Generated -> Indexed(238); Archive 166 -> 125; Docs 30 -> 23.
- fixed: dark Generated / Hidden DarkGray -> Indexed(246) / Indexed(250, italic); Folder LightBlue -> Indexed(75) and Native LightRed -> Indexed(203) (the new test showed the xterm default blue and red under 4.5:1 on #300a24).
- fixed: dracula Generated #8d9bc8 -> #a3a3ad (5.70 bg, 3.66 selection), synthwave #9d8bd0 -> #a9a9b6 (7.87, 5.12): neutral grey versus the lavender Hidden.
- not changed: focused-row behaviour.
- Tests: `-- ui:: theme:: explorer` 159 passed, 2 ignored; fmt and clippy -D warnings clean; .ans preview and contrast.md regenerated once (temporary test removed).
- Rule rows touched: AC-F5 (dark / light) now pass; BASE-3 pass (red seen first this round); BASE-25, BASE-46 re-run on final code.
