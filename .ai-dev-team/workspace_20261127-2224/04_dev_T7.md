# T7 Agent notify mark (D9, SEC-14) — worktree .worktrees/T7, branch run4-T7 (base c8675ed). Status: DONE

Tools available: Agent, Artifact, Bash, Edit, Read, Write, Skill, ToolSearch, SubagentHandback.

## Progress
- read spec D9/AC-10, SEC-14, ui-spec S4, mockups, T5 notes; alacritty_terminal exposes only `Event::Bell`; OSC 9 / 777 are dropped by the emulator -> own byte scanner.
- red: vr-term tests did not compile (no `take_attention`); green after `attention.rs` + terminal wiring.
- ui: `ai/attention.rs` (collect, mark, clear_viewed, notice text, badge), `AiSession.notify`, hooks in `ai::refresh`, `ui::render_with`, `status_bar::render`.
- red -> green for vr-ide tests (two first runs failed on test mistakes: open_tool already shows the panel; rate limit withheld the 2nd notice).
- snapshots generated once at the end; guard green.

## Files changed
- vr-term: `src/attention.rs` (new scanner: BEL, OSC 9, OSC 777;notify, ST/BEL/CAN/SUB, DCS/APC skipped, body cap 1 KiB; OSC 9;4 progress ignored; a BEL that ends an OSC is not a bell), `src/terminal.rs` (`Terminal::take_attention`, queue cap 64, wake on empty -> non-empty edge, not coalesced with output because a hidden panel never re-arms the snapshot wake), `src/lib.rs` (`Attention`), `tests/attention.rs`.
- vr-ide: `src/ai/attention.rs` (new), `src/ai/mod.rs` (`AiSession.notify`, `mod attention`, `refresh` calls `collect`), `src/commands/terminal.rs` (`TermTab::take_attention`), `src/ui/mod.rs` (`clear_viewed` before layout), `src/ui/status_bar.rs` (badge overlay; `render` wraps `render_bar`), tests: `ai_tests.rs` (3), `ui/tests/ai_view_tests.rs` (1 + `notify` field), 2 snapshots `ai_hidden_notify_{120x40,80x24}`.
- Tab marker in `ui/ai_view.rs`: already drawn by T5 (`mark: attention.is_some()`); not touched (no conflict with T6's `±N`).
- README `#### Agent notifications`.
- Regular terminal-panel tabs never call `take_attention`: no mark, existing bell behaviour (none) kept.

## Behaviour
- Marked when the session is not viewed (viewed = active tab of a shown panel, even unfocused); mark order = oldest first for `ai::attention_target` (f12).
- Notice (Info, no expiry, until viewed or Esc): wide `◆ <label> (<folder>): <msg>  f12 shows it` when status width >= 100 and it fits, else `◆ <label>: <msg> f12` with the message cut by `…` before the key. Message: OSC text (sanitize, 200 chars) or `rang the bell`. One notice per session while marked, and at most one per second per session (the mark is set regardless).
- Badge at the status bar's right end ` ◆ label (+N) ` + ` f12 ` when marked sessions are not on screen (panel hidden or inside `+N`).
- Viewing clears mark, notice, badge (`clear_viewed` each frame).

## Does Claude Code / codex emit BEL / OSC 9? (USER-4: config untouched)
Not verified by running the tools (no network / no tool config changes). From my knowledge: Claude Code `preferredNotifChannel` = `auto` | `terminal_bell` | `iterm2` | `iterm2_with_bell` | `notifications_disabled`; `auto` notifies only in terminals it recognises, and vr-ide presents `TERM=xterm-256color` with the outer-terminal variables removed, so the user may need `terminal_bell`. Codex: `[tui]` `notifications` / `notification_method` (`auto`/`osc9`/`bel`) in `~/.codex/config.toml`. README says this with a "names change between versions" caveat.

## Commands (trimmed)
- red: `cargo test -p vr-term` -> `error[E0599] no method take_attention`.
- `cargo test -p vr-term` -> ok 18 + 2 (attention) + 1 + 1 + 6 + 21 passed.
- `cargo test -p vr-ide ai::attention ai_tests::{a_hidden,the_bell,notification}` -> 5 passed.
- `INSTA_UPDATE=no run-heavy.sh cargo test -p vr-term -p vr-ide` -> vr-ide 742 passed, 1 failed (`app_tests::external_modify_reloads_within_one_second_with_real_watcher`, real-watcher timing under load; alone `ok. 1 passed`).
- Guard `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` -> `Finished dev profile`, exit 0.
- Formatting only with rustfmt on the changed files.

## Deviations
- Status badge overlays the right end of the status text (it may cover the position text on narrow widths) rather than reflowing it.
- Wide/narrow chosen by threshold 100 cols + fit (mockup 80x24 is the narrow form because of the `(+1 more · Esc)` suffix).

## Out of scope
- `external_modify_reloads_within_one_second_with_real_watcher` flaky under whole-crate load.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| OSC / bell detection | alacritty_terminal events, marker.rs | Bell only in emulator; own small scanner modelled on `MarkerScan` (raw bytes before the parser) |
| Sanitizing, notices, tab mark, f12 hook | safe_text, `App::push_notice`, T5 `AiSession.attention` / `attention_target`, `ai_view` mark | reused |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-10 / D9 mark + notice on BEL / OSC 9 / OSC 777 | pass | vr-term `attention::tests::bell_and_both_notifications_are_found`, `tests/attention.rs`; `ai_tests::a_hidden_session_is_marked_noticed_and_f12_shows_it_and_clears` |
| AC-10 while hidden: f12 opens the marked session; viewing clears | pass | same test; `ai_tests::the_bell_marks_with_its_own_words_and_a_viewed_tab_is_not_marked` |
| S4 wording wide / narrow, badge | pass | `ai::attention::tests::notice_forms_follow_the_mockups`; snapshots `ai_hidden_notify_*`, `ai_view_tests::ai_hidden_notify`; tab `◆` T5 snapshot `ai_panel_*` |
| SEC-14 sanitize, cap, label prefix, rate limit, flood | pass | `ai_tests::notification_text_is_sanitized_capped_and_rate_limited` (ESC[2J + 10 KiB, 1000 bells -> one notice), `message_is_sanitized_capped_and_never_empty`, `tests/attention.rs::a_flood_keeps_a_bounded_queue…` |
| Regular terminal tabs do not mark | pass | `take_attention` called only from `ai::attention::collect` |
| USER-4 tool config untouched, README states settings | pass | README `#### Agent notifications` |
| BASE-11/24/25/38, COST | pass | no git state changes, nothing staged; guard exit 0; related tests only + one snapshot generation |

## Fix round 1
1. Badge reflow: `ai::attention::badge(app, width)` picks a tier (` ◆ label +N ` + ` f12 `, else ` ◆ ` + ` f12 `, else none when < 8 cells would remain); `status_bar::render` draws the bar (all paths: file, AI focus, terminal focus) into the width left, then the badge. Test `ai_view_tests::badge_reflows_the_status_text` (80 cols, long path). The two `ai_hidden_notify` snapshots regenerated: unchanged (short path).
2. `a_long_body_is_capped` + `an_aborted_string_gives_no_text_and_its_bell_byte_counts_as_a_bell` (exact `[None]`, comment why).
3. DCS/SOS/PM/APC end only at ST / CAN / SUB (BEL is payload, as in vte); ESC inside OSC or a skipped string re-dispatches (`after_esc`). Test `bells_inside_a_dcs_are_payload`.
4. `SpawnExtras.attention` (AI sessions true; ssh, shells false): no scan, no queue, no wake. Test `a_terminal_that_is_not_watched_queues_nothing…`; the flood test made deterministic.
5. OSC 9;<1-2 digits>[;…] ignored (ConEmu); `9;5 files done` still a message. Test `conemu_sub_commands_are_no_notification`.
6. Comment moved above `kept`.
Commands: guard (fmt + clippy -D warnings) exit 0; `cargo test -p vr-term` all green (21 + 3 attention + others); `cargo test -p vr-ide` 743 passed, 1 failed (`search_tests::project_replace_never_loses_a_concurrent_write`, timing under load; alone `ok. 1 passed`).
