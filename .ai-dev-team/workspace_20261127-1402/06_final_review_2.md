# Final review 2 — 9c45cd1..HEAD (cross-task: correctness + security + UI)

Verdict: **FINDINGS** (1 major, 4 minor). Release binary `target/release/vr-ide` (2026-11-26 00:09) is newer than every source file; live walks used it via the FR-10 pty harness (scripts kept in the reviewer's scratchpad, q1.py / q2.py / q3.py).

## Findings (ranked)

[major] crates/vr-ide/src/commands/app.rs:28 + crates/vr-ide/src/git_popup.rs:1282 — Quit cancels a running push / pull / commit without asking or saying so
Scenario (verified live, both cases): `/git push` → Push → Esc (popup hidden, status bar shows only `main ↑1`, nothing says a push runs) → ctrl+q. vr-ide exits in 0.0 s, the push is killed, the remote is not updated (checked again after the hook would have finished), no `vr-ide:` line on stderr. Same with a commit held by a pre-commit hook: exit, no commit, the typed message is gone, nothing printed. Terminals with a running program ask first (T26, `terminal::quit_allowed`), a running replace refuses (D129); a git write is the only background job that quit drops silently. D306 accepted "exit cancels", not "without telling".
Fix: in `quit` / `quit_discarding` / `answer_open_workspace`-style gates, add `git_popup::quit_allowed(app)` that, when `p.job` or `p.commit_cancel` is set, opens "Git push is running. Quit cancels it." [Cancel (default)] [Quit anyway]; on Quit anyway push an `exit_messages` line ("git push to origin/main was cancelled"). Show a status-bar segment (`push…` / `commit…`) while the popup is hidden and a job runs.

[minor] crates/vr-ide/src/commands/explorer.rs:1103 (`rename_in`) / :1118 (`retarget`) — Explorer rename and move leave /recent on the old path
Scenario (verified live): open a.txt, ctrl+b, Down, F2, `b.txt`, Enter → `recent-files.toml` still `files = [".../a.txt"]`, `/recent ` lists `1 a.txt`; choosing it fails on a missing file. Only `/rename` calls `recent_file_renamed` (slash.rs:1142, the T20b fix); F2, the T23 menu's Rename and cut+paste moves do not.
Fix: call `app.recent_file_renamed(old, new)` for each moved tab path inside `retarget` (and drop the call in slash.rs), so every rename/move path updates it.

[minor] crates/vr-ide/src/monitor.rs:367 — Welcome branch read bypasses vr-git's SEC-6 / SEC-15
Scenario (static): `git_branch` climbs every ancestor with no filesystem-boundary or owner check and follows any `gitdir:`; the welcome (welcome.rs:388, 453, 550) shows the branch of a `$HOME` dotfiles repo or a planted `/tmp/.git` that vr-git refuses (the status bar then shows no branch: two answers on one screen). The text is not passed through `sanitize`; only ratatui's control/zero-width filter stands between a crafted HEAD and the screen.
Fix: take the branch from `app.git.repo_info_of_root(root)` (already sanitized, SEC-6 discovery) and drop `monitor::git_branch` (also removes a duplicate reader, BASE-16).

[minor] crates/vr-ide/src/config_reload.rs:172 — Live keymap reload skips the unreachable-key check
Scenario (static): on a legacy terminal, adding `"ctrl+shift+g" = "file.goto"` (replacing ctrl+p) to keymap.toml gives "keymap.toml applied"; the key can never arrive and no notice says so, whereas at startup `report_keyboard` (app.rs:436) lists such keys.
Fix: after `self.keymap = keymap`, run the lost-key part of `report_keyboard(self.keyboard_enhanced)`.

[minor] leftovers — crates/vr-ide/src/commands/explorer.rs:1135 `retarget` does not `git.forget_doc(old)` (marks of the old path stay in `git.marks` until close); crates/vr-ide/src/theme.rs:49 comment still says "side panel (explorer or search)" though the search side panel is gone.

## Checked, no finding
Modal routing: keys and mouse both go dialog → git popup → explorer menu → palette → terminal → find/search, in the same order the overlays are drawn (ui/mod.rs:73-80); the explorer menu closes on any other key and on focus loss; paste follows the same order. Workspace switch: tabs closed, index/search/explorer watches/git roots/popup (orphaned) follow `roots_changed`; stale results cleared; cut paths from the old workspace refused by `below_root`. Exit order: restore terminal → recovery → recent-files writer (500 ms) → terminals → git popup (3 s) → git → replace wait. Config watches survive closing a config tab (`close_doc` keeps them). /rename and explorer ops stay confined per root. Removed features: no leader, `terminal.new/close`, `/term new`, `/wrap` left in code, docs or defaults; docs/commands.md matches the slash table.

## Files read
crates/vr-ide/src/{app.rs (routing, watches, tick, close_doc, recent), app_workspace.rs, event_loop.rs, main.rs, config_reload.rs, git.rs (schedule, status_label, roots_changed, shutdown), git_popup.rs (close_session, shutdown, handle_key, roots_changed), monitor.rs (git_branch), commands/{app.rs (quit), explorer_menu.rs, explorer.rs (confinement, rename, retarget), slash.rs (table, rename), search.rs (handle_key)}, ui/{mod.rs, layout.rs, welcome.rs (branch)}}, crates/vr-core/src/watch.rs (watch/unwatch), crates/vr-config/defaults/keymap.toml, docs/keymap.md, docs/commands.md — read (cross-task paths only; every task diff already had its own review).
