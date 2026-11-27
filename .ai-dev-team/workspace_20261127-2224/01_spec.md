---
feature: Run 4 — session restore, desktop launcher, SSH argument line, Tab completion, AI panel
has_ui: true
security_sensitive: true   # ssh argument line; reverting files an AI agent changed (overwrites user files)
status: approved scope (user 2026-11-27 13:50), design gate open
---

# Run 4 spec

User request (2026-11-27 13:46–13:52, Vietnamese, summarised):
"Keep the session (e.g. added workspace folders); add an app icon to open the IDE instead of typing
the command in GNOME Terminal; default workspace is the user's ~/." Then: "AI code features — a
console for Claude Code / Cursor … an extension dedicated to AI coding instead of working in a plain
terminal." Picks: AI panel, see what the AI changed, notify when the agent needs me; panel on the
right, full height. "Fix the ssh tool so it runs like in a terminal, e.g.
`ssh -i credentials/id_fixecom root@app.fixecom.vn`, and support Tab auto-complete in tools where
text is typed freely." "Draw the mockups first; I approve, then the build starts and I leave the
machine." Commit + reinstall + push at the end.

## Decisions
- D1 (user) Restore = workspace folders + open file tabs (cursor, scroll, read/edit mode) + active
  tab + explorer expanded folders + terminal / AI panel layout. Terminals restart as a fresh shell.
- D2 (user) Only the icon launch restores: the icon runs `vr-ide --session`; with no saved session
  it opens `~/`. `vr-ide` typed in a console behaves exactly as today (current folder, no restore,
  does not overwrite the saved session). `vr-ide <folder|file|.vr-workspace>` unchanged.
- D3 (leader) The session is saved by a `--session` instance only: on quit and debounced after each
  change (≤ 2 s), so a crash or a closed window keeps it. File `~/.local/state/vr-ide/session.toml`,
  mode 0600, atomic write. Unsaved buffer text is never stored (quit already asks). A missing root
  is kept and shown `(missing)` as today; a missing file tab is skipped with one notice.
- D4 (leader) Launcher: `vr-ide --install-desktop` writes `~/.local/share/applications/vr-ide.desktop`
  and the icon into `~/.local/share/icons/hicolor/scalable/apps/vr-ide.svg`; `--uninstall-desktop`
  removes exactly those files. The entry starts vr-ide in a terminal window (GNOME Terminal when
  present, else `Terminal=true`), titled vr-ide, `--session`. It is vr-ide's own entry — no other
  application's settings change (USER-4).
- D5 (user) `/ssh` takes an ssh argument line as typed after `ssh ` in a terminal
  (`-i credentials/id_fixecom root@app.fixecom.vn`, `-p 2222 user@host`, `-J jump host`). Relative
  paths resolve like in a terminal opened in the active file's workspace folder. Split shell-like
  (quotes, backslash), never run through a shell. The recent list keeps the argument line.
  Everything FR-25 gave (session tab, full screen after login, status row) keeps working.
- D6 (user) Tab completion in every palette input that takes free text: after `/` the command
  names; path arguments (`/ws add`, `/ws open`, `/ws save`, `/ssh -i|-F|-o IdentityFile=`, `/run`
  file arguments) complete paths; `/ssh` host position completes `~/.ssh/config` hosts + recent
  targets; `/ai` completes tool names. One match → completes; several → longest common prefix, a
  second Tab shows the candidate list, Tab / Shift+Tab step it.
- D7 (user) AI panel: a column right of the editor, full window height (the terminal panel stays
  below the editor). One tab per agent. Tabs are agent sessions: several sessions of the same tool may run at once
  (user 14:07); labels tell them apart. Tools from setting `ai_tools` (default claude, codex,
  gemini, aider; only installed ones offered; cursor-agent when installed). A new agent starts in
  the active file's workspace folder. A key shows / hides the panel; agents keep running hidden.
  `/ai <tool>` opens in this panel. On a session restore, agent tabs reopen with each tool's
  continue arguments (setting per tool; claude `--continue`, codex `resume --last`, …).
- D8 (user) AI changes: while an agent runs, files in its workspace changed by anything other than
  vr-ide's own editor are recorded with their content before the first change (pre-image, kept
  under `~/.local/state/vr-ide/ai/`, size-capped). A list shows the changed files per agent; a file
  opens as a diff (pre-image vs now) in the editor; Keep drops the record; Revert writes the
  pre-image back (asks first; refuses when the file changed again since the diff was shown;
  atomic write; a created file is deleted to the trash, never unlinked). Changes by other programs
  in that window are attributed to the agent too — stated in the UI.
- D9 (user) Notify: an agent tab is marked, and a notice shows, when the agent's program rings the
  bell or sends a desktop-notification escape (OSC 9 / OSC 777), also while the panel is hidden;
  viewing the tab clears it.
- D10 (user, overrides USER-3 for this run) The UI is designed as mockups first (ANSI `.ans`
  viewable with `cat`, USER-2), the user approves, then the build starts. Keys follow USER-5;
  the designer proposes them in the mockup's key table.

## Acceptance criteria
- AC-1 Icon launch with no session opens `~/` as the only workspace folder.
- AC-2 Icon launch after a session restores D1's items; files edited outside since are read fresh.
- AC-3 Console `vr-ide` (no args) opens the current folder and never reads or writes the session.
- AC-4 Session file is written atomically, 0600, on quit and ≤ 2 s after a change; a corrupt file
  gives one notice and an empty session (opens `~/`), never a crash.
- AC-5 `vr-ide --install-desktop` creates the entry + icon; it shows in GNOME's app grid and opens
  vr-ide in a terminal window; `--uninstall-desktop` removes only those files.
- AC-6 `/ssh -i credentials/id_fixecom root@app.fixecom.vn` starts the same ssh command a terminal
  would (path resolved against the active file's workspace folder); quoting works; recent keeps it.
- AC-7 Tab completion per D6 in the palette, at 80x24 and 120x40.
- AC-8 AI panel per D7: open, several agents, hide/show without stopping them, resize width,
  `/ai` uses it, restore with continue arguments.
- AC-9 AI changes per D8: list, diff, keep, revert with the safety rules.
- AC-10 Notify per D9.
- AC-11 Existing features unchanged (suite green, FR-17..FR-33 behaviour).
- D11 (user 14:09, option 2) Accounts per agent session, without cswap: vr-ide keeps one config
  folder per account (`~/.local/state/vr-ide/accounts/<name>/`, 0700) and starts a claude session
  with `CLAUDE_CONFIG_DIR` set to it (codex with `CODEX_HOME` when that tool is used the same way).
  The account is picked when a session starts and shown in its tab label; a new account is added by
  name and logged in once with `/login` inside the tab. vr-ide never reads, copies or stores tokens
  itself; no usage tracking, no auto-rotation. "Default" = the tool's normal config (no variable).
- AC-12 Two claude sessions on two accounts run side by side; each tab shows its account; restore
  reopens each on its account; removing an account asks and deletes only its folder.
- D14 (user 14:36) Starting a session asks: New / Continue last / Choose an earlier one. Continue
  and Choose use the tool's own resume (claude `--continue` / `--resume` opens claude's own list
  in the tab; codex `resume --last` / `resume`); tools without resume show only New. vr-ide does
  not read the tools' history files. Settings per tool hold these arguments (with D7's continue
  arguments used by restore).
- AC-13 The picker's second step offers New / Continue last / Choose earlier for tools that
  support it; each starts the tool with the matching arguments.
