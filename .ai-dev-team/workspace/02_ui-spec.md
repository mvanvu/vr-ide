# UI Spec: run 4 — AI panel, AI changes, notify, palette Tab completion, restore notices

platforms: terminal (ratatui TUI) · sizes 120x40, 80x24, 40x12 · theme shown: `synthwave` (default)
Source of truth for the build: the `.ans` mockups in `$TEAM_WS/mockups/` (USER-2) plus this file.
Generator (throwaway): `$TEAM_WS/qa-artifacts/DS/screens.py` + `ans.py`; width check `check.py`.

View them in the terminal (truecolor):

```sh
cd .ai-dev-team/workspace/mockups
for f in ai-panel-A-120x40 ai-panel-B-120x40 ai-picker-120x40 ai-changes-A-120x40 ai-changes-B-120x40; do
  clear; cat $f.ans; read -p "$f — enter for next"; done
```

## Decisions this spec takes (for the user's approval)

| # | Choice | Recommended | Why |
|---|---|---|---|
| L1 | AI panel vs a terminal set to the right (`terminal_position = "right"`) | **A**: while the AI panel is shown, the terminal panel sits at the bottom of the editor column; the setting is kept and the terminal goes back to the right when the AI panel hides | D7 asks for a full-height AI column; a second right column (B) halves the agent's height and adds a third vertical split at 120 cols; the same fallback already exists (a right terminal drops to the bottom under 60 cols) |
| L2 | Where the AI changes list lives | **A**: inside the session's tab, at the top of the panel (`f6` opens / closes it), diff in the editor area | the list sits next to the session that made the changes (D8 "per agent"); the explorer stays; the agent stays visible under the list. B (side column, all sessions grouped) is shown as the alternative |

Everything else has one design.

## Main tasks

| Task | Start point | Steps (A) | Steps (B) |
|---|---|---|---|
| T1 Start the first agent session | editor, panel hidden, none open | 2: `f12` (panel + picker, `claude default` preselected) · `enter` | 2 |
| T2 Another claude session on account `work` | editor, panel hidden or shown | 3: `ctrl+f12` (picker) · `↓` · `enter` | 3 |
| T3 Answer a session that needs you while the panel is hidden | anywhere | 1: `f12` (shows the panel on the marked session, focused) | 1 |
| T4 Review AI changes and revert one file | editor | 6: `f6` · `↓` · `enter` (diff) · `shift+f5` · `←` (Revert; Cancel is the default) · `enter` | 6 (list in the side column) |
| T5 Connect `/ssh -i credentials/id_fixecom root@app.fixecom.vn` | editor | 6: `f1` · type `/ssh -i cr` · `tab` (→ `credentials/id_fixecom`) · type ` ro` · `tab` (→ `root@app.fixecom.vn`) · `enter` | – |
| T6 Add an account and sign in | editor | 7: `ctrl+f12` · `↓`×2 to `+ Add account…` · `enter` · type the name · `enter` (session starts on it) · type `/login` + `enter` in the tab | 7 |

Option B of L1 changes no step count (it trades the agent's height for a visible terminal); option B
of L2 changes none either (it trades the explorer for a wider, all-sessions list).

## Inventory

- Product type: a terminal IDE (ratatui) with an integrated terminal; this run adds a right-hand AI
  column hosting terminal-based coding agents (claude, codex, gemini, aider, cursor-agent).
- Chrome in play: explorer (left), editor tab bar, editor, terminal panel (bottom; right is a setting),
  notice row (full width, above the status bar), status bar (full width), palette overlay, dialogs.
- New regions: AI panel (separator column, tab bar row, agent grid), session picker (in the panel),
  changes list (in the panel, L2-A), diff tab (editor area), attention badge (status bar right).
- Content set used in every mockup: workspace roots `vr-ide` (open) and `shop`; file
  `crates/vr-ide/src/session.rs`; sessions `1 claude` (default account, vr-ide), `2 claude@work`
  (vr-ide, needs attention), `3 codex` (shop); changes of `claude`: `M session.rs +12 -3`,
  `A session_tests.rs +88`, `D docs/old-session.md -40`; ssh key `credentials/id_fixecom` in
  `~/DEV/fixecom`, host `root@app.fixecom.vn`; home shown as `/home/user`.

## Layout (all screens)

```
┌ explorer ─┬ editor tabs ───────────────┬│ AI tabs ──────────┐ row 0
│           │ editor                     ││ agent grid        │
│           │                            ││ (black, green)    │
│           ├ terminal tabs ─────────────┤│                   │
│           │ terminal                   ││                   │
├───────────┴────────────────────────────┴┴───────────────────┤ notice rows (when any)
└ status bar (full width) ────────────────────────────────────┘
```

- AI panel: full height from row 0 to the row above the notices; a 1-column separator at its left
  (`│`, `dim`; `focus` cyan bold while the panel has the keys). The terminal panel and the find box
  live in the editor column only.
- Width: setting `ai_width_percent` (default 40, 10..=90) of the screen right of the explorer, at least
  30 columns; the editor keeps at least 30 columns. Drag the separator, or `ctrl+f8` / `ctrl+f9` in the
  focused panel. The width is part of the restored session (D1).
- Under 60 columns right of the explorer (40x12, or 80x24 with a wide explorer) the shown panel takes
  that whole area and the editor is not drawn until the panel hides (`f12`) — like the SSH full screen.
- L1-A: with the AI panel shown, a terminal panel whose position is `right` is drawn at the bottom of
  the editor column (setting unchanged; `ctrl+f7` still flips it and takes effect when the AI panel
  hides). Mockup `ai-panel-A-120x40`. L1-B (alternative): the right column splits, AI on top
  (≥ 60 %), terminal below with its own tab bar. Mockup `ai-panel-B-120x40`.

## Screens

### S1 AI panel (D7, D11) — `ai-panel-A-120x40`, `ai-panel-A-80x24`, `ai-panel-B-120x40`, `ai-panel-40x12`

- Entry: `f12` (hidden → shown + focused; shown, not focused → focused; focused → hidden, sessions keep
  running, the keys go back to the editor), `ctrl+f12` (picker), `/ai <tool> [@account]`, palette.
- Tab bar (row 0, `tab_bar` surface): one tab per **session**, in start order, each led by its position
  number (`f7` / `shift+f7` step them). Label = `n tool[@account][·k]`:
  - `@account` only for a named account (`default` adds nothing): `2 claude@work`;
  - `·k` numbers the 2nd, 3rd … session of the same tool + account: `1 claude`, `4 claude·2`,
    `5 claude@work·2`; a renamed session (`AI: Rename Session…`) shows `n name[@account]`;
  - the folder (dim) follows only when the session's folder differs from the first session's: `3 codex shop`;
  - `◆` (attention, yellow bold) after the label; `x` (dim) on the active tab; `+` after the tabs opens
    the picker; `±N` (cyan, right end) = the active session's change count, a click opens the list.
  - Fit rule (`ai-panel-A-120x40`, `ai-panel-A-80x24` with 6 sessions), applied in order until the
    tabs fit: 1) inactive tabs drop the folder; 2) the active tab's `x` and the `±N` give way (close:
    palette, or exit the program) and inactive tabs show only what tells them apart (`2 @work`, `4 ·2`;
    tool name when there is nothing else); 3) inactive tabs show their number only (`2 ◆`); 4) the last
    inactive tabs go into `+N` at the right — the active tab and marked tabs are never hidden (a marked
    tab beyond the bar shows `+N◆`). The active tab always shows its full label.
- Grid: the session's terminal on the panel colours (black, Homebrew green, the theme's ANSI table) —
  the same renderer as the terminal panel.
- Focus: separator cyan bold + status bar `AI` badge (new token `mode_ai`, purple) with
  `label · folder` left and `±N f6 · i/n · f12 hide` right; without changes `i/n · f12 hide`.
- States: running (as drawn); exited — label red (`tab_warning`), last output kept, a final line
  `[exited 0] enter: start again (claude --continue)`; restored — new grid with the tool's continue
  command (`restore-missing-120x40`); exited with the changes list open and empty: `ai-exited-empty-80x24` (status `claude [exited 0]` … `enter restart`); hidden — nothing drawn, sessions run, attention per S4.
- 80x24: drawn without the explorer (panel 32, editor 48). With the explorer shown: explorer 20,
  editor 30, panel 30. 40x12: panel takes the area right of the explorer (24 cols); tab bar at fit
  step 3–4; status badge shrinks to ` A `; the explorer's separator turns cyan when the panel has the keys (no second separator). Mockup `ai-panel-40x12`.

### S2 Session picker, accounts (D7, D11) — `ai-picker-120x40`, `ai-empty-80x24`, `ai-account-add-120x40`, `ai-account-login-80x24`, `ai-account-remove-80x24`, `ai-account-busy-80x24`

- Shown in a new tab (`n new`) of the panel by `ctrl+f12` / `+` / `AI: New Session…`, and as the empty
  state when the panel opens with no session (tab `AI`, title `Start in <folder>`).
- Title `New session in <folder>` (the active file's workspace folder, cyan).
- Rows grouped by tool (pink bold header; installed tools from `ai_tools` only); under a tool that
  supports accounts (claude via `CLAUDE_CONFIG_DIR`, codex via `CODEX_HOME`): `default`, each named
  account (purple), then `+ Add account…` (`note`). Right column: `n open` or `new`. A tool
  without account support is one row (`gemini   new`).
- Under the list: `Not on PATH: aider, cursor-agent` and `(ai_tools in settings)` in `note`;
  no tool at all: `No AI tool found on PATH (ai_tools: claude, codex, gemini, aider)`.
- Footer row: `enter start  del remove  esc back` (keys orange, words dim); empty state: `esc hide`.
- Preselected row: the active session's tool + account (else the first `default`).
- Add account (`ai-account-add-120x40`): `enter` on `+ Add account…` turns the row into a field
  `name [personal▌]` with the rule under it (`a-z 0-9 -, for claude`); a bad or taken name
  shows the error in that line (red) and keeps the text; `enter` creates
  `~/.local/state/vr-ide/accounts/<name>/` (0700) and starts the session on it; `esc` cancels.
  The new tab (`4 claude@personal`) shows the tool's own sign-in text; notice
  `Account personal created; sign in with /login in its tab` (info). vr-ide never touches the login.
- Remove account (`ai-account-remove-80x24`): `delete` on a named account row → dialog
  `Remove account <name>?` / `Deletes ~/.local/state/vr-ide/accounts/<name>/ for good: its claude login,
  settings and history.` / `The sign-in stays valid at the provider; to end it, run /logout in a session
  on <name> first.` / `Other accounts and claude's own config stay.` / `[Remove] [Cancel]`, Cancel default
  (SEC-17: deleted, not trashed). With sessions open on it the dialog is not shown; warning notice
  `Not removed: close the session on work first, tab 2` (plural: `close the 2 sessions on work first,
  tabs 2, 5`) — `ai-account-busy-80x24`. `default` cannot be removed (`delete` does nothing,
  hint `default is claude's own config`).

### S3 AI changes (D8) — `ai-changes-A-120x40`, `ai-changes-A-80x24`, `ai-changes-B-120x40`, `ai-revert-confirm-120x40`, `ai-revert-created-80x24`, `ai-revert-refused-80x24`, `ai-changes-limits-120x40`, `ai-exited-empty-80x24`

- L2-A list: `f6` (or a click on `±N`) opens it at the top of the active session's tab and focuses it;
  `f6` / `esc` closes it. Header row (`raised`): `Changes · <session> · N files` … `f6`. One row per
  file: kind letter (`M` cyan / `A` green / `D` red — the `git_*` tokens), file name (fg), parent
  folder (`note`, cut from the left with `…`, dropped under 10 cells), counts right (dim).
  Rows take up to a third of the panel (at least 3; the list scrolls). Under the rows, always, in
  `note` italic: `Counts edits by any program in` / `<folder> while <session> runs` (D8 statement).
  A `─` rule, then the agent grid continues below.
  Empty: `No changes since it started` (one row, `note` italic; `ai-exited-empty-80x24`).
- Rows without an earlier version (`ai-changes-limits-120x40`), counts column in orange:
  - `no earlier` (SEC-9): vr-ide first saw the file after the change. Diff tab header
    `M .env  changed, no earlier version kept` and only `f5 keep` at the right; body = two `note` italic
    lines `vr-ide saw .env only after claude changed it:` / `revert is off; this is the file as it is now.`
    then the current text without +/- signs; status `… · changed, no earlier version kept` …
    `i/n · f5 keep · revert off`. `shift+f5` does nothing but show the notice
    `No earlier version of .env kept; Keep only` (warning).
  - `too large` (SEC-10, over 8 MiB) and `limit` (SEC-10, the session's 256 MiB / 5 000 records or the
    512 MiB total reached): same diff-tab treatment with the header `too large, no earlier version kept`
    / `limit reached, no earlier version kept`; Keep only.
- Incomplete list (SEC-13): after a watcher error or rescan, a banner under the header, orange:
  `! List may be incomplete (watcher` / `rescan at 14:41) · enter hides` — it is the list's first row,
  `enter` on it dismisses it (until the next error); it never hides by itself.
- L2-B list (alternative): the explorer column switches to `AI CHANGES` (all sessions, grouped with
  `▾ <session> <folder> · N`); `f6` switches back to the explorer.
- Keys in the list: `↑`/`↓`, `enter` opens the diff tab (list stays), `f5` keep, `shift+f5` revert.
- Diff tab (editor area, read-only, read mode): tab label `± <file name>`; header row (`raised`)
  `M <path>  +12 -3 · claude` and, right, `f5 keep  shift+f5 revert` (orange; dropped when narrow —
  the status bar keeps them). Body: unified diff, hunks headed `@@ -a,b +c,d @@ <context>` (purple
  italic), each line `nnn ±code` with syntax colours on new tokens `diff_added_bg` / `diff_removed_bg`;
  `+` green, `-` red signs (never colour alone). Created file: all `+`, header `A … new · +88`;
  deleted file: all `-`. Status bar: `DIFF` badge (`mode_ai`) · path · session · `since 14:32` …
  `i/n · f5 keep · shift+f5 revert`. Find (`ctrl+f`) works in it.
- Keep: no dialog; the row leaves the list, the diff tab closes, notice `Kept session.rs` (info).
- Revert: dialog (`ai-revert-confirm-120x40`) `Revert session.rs?` / `Writes back session.rs as it was
  before claude changed it (14:32). Today's text, +12 -3, is lost.` / `Edits by other programs since 14:32 count
  as claude's.` (`note`, SEC-12) / `[Revert] [Cancel]`, Cancel default. Created file
  (`ai-revert-created-80x24`): `claude created this file. Revert moves it to the trash (88 lines).` /
  `Other programs' edits there count as claude's.` Deleted file: `Restores old-session.md (40 lines) that claude deleted.`
- Refusal (`ai-revert-refused-80x24`): when the file differs from the state the diff (or the dialog)
  showed, nothing is written; the diff reloads and the notice says
  `Not reverted: session.rs changed since shown; diff updated` (warning). The user looks and reverts again.
- An open editor tab of a reverted file reloads like any outside change (existing rule).
- Two sessions in one folder: each keeps its own record of the same file; Keep / Revert act on the
  session whose list it is.

### S4 Attention mark (D9) — `ai-panel-A-120x40` (tab `◆`), `ai-hidden-notify-120x40`, `ai-hidden-notify-80x24`

- Trigger: BEL, OSC 9 or OSC 777 from a session that is not the one being viewed (viewed = active tab
  of a shown panel).
- Tab: `◆` after the label (yellow `attention` token, bold) — a glyph, not colour alone, distinct from
  the editor's modified `●`.
- Notice (info, one per session until viewed): `◆ <tab label> (<folder>): <message>  f12 shows it` at
  120 cols, `◆ <tab label>: <message> f12` when narrow (folder and `shows it` go first; the message is cut
  with `…` before `f12`); the tab label is the full one (`claude@work`, without the number). The
  message is the OSC 9 / 777 text (control characters removed, cut with `…`), or `rang the bell`.
- Panel hidden (or the marked tab inside `+N`): status bar right end gets ` ◆ claude@work ` (yellow
  background, bold, `+N` when several) followed by ` f12 `.
- `f12` shows the panel on the oldest marked session; viewing a tab clears its mark, its badge part
  and its notice.

### S5 Palette Tab completion (D5, D6) — `palette-tab-prefix-80x24`, `palette-tab-list-120x40`, `palette-tab-list-80x24`, `palette-ssh-path-120x40`, `palette-ssh-host-80x24`, `palette-ssh-error-80x24`, `palette-ssh-recent-80x24`

- One match: `tab` completes in place (a folder ends with `/`, a finished argument gets a space).
- Several: `tab` inserts the common part; the input row shows `tab: list (N)` (dim, right) and the
  detail area lists them in one line when it fits (`palette-tab-prefix-80x24`). No common part to add:
  the first `tab` opens the list at once.
- Second `tab`: the palette's list rows become the candidates (folders purple `/`, files fg, hosts with
  their source `recent` / `~/.ssh/config` right-aligned); the input shows the selected candidate;
  `i/N` right of the input. `tab` / `shift+tab` step (wrapping), `enter` takes it and closes the list
  (does not run), `esc` restores the text from before the list, typing closes the list and edits.
  Detail line: `tab next  shift+tab previous  enter take  esc back to <prefix>`.
- Sources: after `/` the command names; `/ws add` folders; `/ws open` folders and `.vr-workspace`
  files; `/ws save` paths; `/ssh -i|-F|-o IdentityFile=` files relative to the active file's workspace
  folder (detail `files in ~/DEV/fixecom (workspace folder of the active file)`); `/ssh` host position
  `~/.ssh/config` hosts + recent targets; `/run` file arguments; `/ai` tool names and `@account`.
  Dotfiles only when the typed part starts with `.`.
- `/ssh` argument line: under the candidates the detail shows `runs: ssh <args with resolved paths>`,
  wrapped at spaces and after `/` (never inside a name; continuation lines indented under the command;
  `palette-ssh-host-80x24`) — the exact command, never through a shell. A split error is the
  palette's slash error row (red): `/ssh: unclosed " (quote paths with spaces, or use \ )`.
- Recent `/ssh` rows (SEC-5, `palette-ssh-recent-80x24`): the whole argument line, sanitized; when it does
  not fit, cut in the middle with `…` so the first option and the destination stay
  (`-o ProxyCommand="ssh -W %h…" deploy@db1`). Selecting a row whose line has a local-command word
  (`…Command`, `PKCS11Provider`, `SecurityKeyProvider`, `-I`) fills the input only and the detail says, orange
  bold: `Has a local-command option (ProxyCommand):` / `check it; enter runs it`. Other rows connect as before.
- 40x12: the box is 36 wide, list ≥ 1 row; the counter stays; the detail rows go first.

### S6 Session restore notices (D2, D3) — `restore-missing-120x40`, `restore-corrupt-80x24`

- Missing file tabs: one warning notice after the restore:
  `Session restored; 2 tabs skipped, files not found: src/old_layout.rs, docs/plan.md` (paths relative
  to their root; more than fit end with `… +3`). Missing roots keep the explorer's `(missing)`.
- Restored agent sessions start with their continue command on their account; the grid shows it.
- Restore refusal (SEC-18, `ai-restore-refused-80x24`): a session whose account folder is missing or
  invalid, whose tool is not in `ai_tools` or not on PATH, or whose root is gone is not reopened — never
  on `default` or another account. One error notice each: `Not reopened: claude@old-team, account folder
  missing (no fallback)`, `Not reopened: aider in ~/DEV/api, aider not on PATH`, `Not reopened: codex in
  ~/DEV/old (folder missing)`. The other sessions reopen.
- Corrupt file: error notice `Session file unreadable (line 4); opened ~/, kept it as session.toml.bad`
  and the `~/` welcome screen. Copying the bad file aside before the next save is a proposal (Q2).

## Key table (USER-5)

All keys reach vr-ide from a legacy terminal; none is alt, ctrl+shift+letter, ctrl+PageUp/Down,
F10/F11 or an ambiguous control byte. Checked against `docs/keymap.md` (taken: ctrl+q p o s w n e b k
h r g f z y c x v a d l /, f1 f2 f4 f8 f9, shift+f2 f4 f8 f9 f12, ctrl+f6 f7 f8 f9, ctrl+`).

| Command | Title | Key | Where it works |
|---|---|---|---|
| `ai.toggle` | AI: Show / Hide Panel | `f12` | global; also taken from a focused terminal and a focused session |
| `ai.new` | AI: New Session… | `ctrl+f12` | global; also from a focused terminal and session (opens the panel on the picker) |
| `focus.editor` | Focus Editor | `f3` | global and focused session (was palette only); not taken from terminals (mc's F3) |
| `ai.changes` | AI: Show Changes | `f6` | global and focused session (not terminals: mc's F6) |
| `ai.next` / `ai.prev` | AI: Next / Previous Session | `f7` / `shift+f7` | global and focused session (not terminals) |
| `ai.keep` / `ai.revert` | AI: Keep / Revert Change | `f5` / `shift+f5` | changes list and diff tab |
| `ai.grow` / `ai.shrink` | AI: Increase / Decrease Panel Width | `ctrl+f8` / `ctrl+f9` | focused panel (same keys as the terminal panel's size) |
| `ai.rename` | AI: Rename Session… | – (palette: opens `/ai rename `) | |
| `ai.close` | AI: Close Session | – (palette, tab `x`; asks first while the program runs) | |
| built-in | picker | `↑` `↓` `enter` `delete` `esc` | session picker |
| built-in | palette completion | `tab` / `shift+tab` | palette input |
| built-in | focus cycle | `tab` in read mode: editor → explorer → terminal → AI panel → editor | |

Why no `ctrl+n` / `ctrl+w` in a session (unlike `[terminal]`): agents' prompt editors use `ctrl+w`
(delete word) and other ctrl letters; a focused session takes only F keys from the program
(see Key plumbing).
The welcome screen's key list gains `f12  AI panel` (`restore-corrupt-80x24`).

## Key plumbing (for T5–T7 and the docs)

- `crates/vr-config/defaults/keymap.toml`, global table: `f12` = `ai.toggle`, `ctrl+f12` = `ai.new`,
  `f3` = `focus.editor`, `f6` = `ai.changes`, `f7` = `ai.next`, `shift+f7` = `ai.prev`.
- `[terminal]` gains `f12` and `ctrl+f12`: a focused terminal keeps every key except nine —
  ``ctrl+` ``, `f1`, `ctrl+f7`, `ctrl+f8`, `ctrl+f9`, `ctrl+n`, `ctrl+w`, `f12`, `ctrl+f12`.
- New table `[ai]` (a focused session; like `[terminal]`, every other key goes to the program):
  `f3`, `f6`, `f7`, `shift+f7`, `f12`, `ctrl+f12`, `ctrl+f8` = `ai.grow`, `ctrl+f9` = `ai.shrink`,
  ``ctrl+` `` = `terminal.toggle`; `f1` is built in as for terminals; NUL follows the existing
  `terminal_pass_nul` rule. Users may add keys to `[ai]`.
- Keys a focused session loses to vr-ide (listed in docs/keymap.md `[ai]`): `f1`, `f3`, `f6`, `f7`,
  `shift+f7`, `f12`, `ctrl+f8`, `ctrl+f9`, `ctrl+f12`, ``ctrl+` ``. `f5` / `shift+f5` and all ctrl
  letters reach the program.
- `f5` = `ai.keep`, `shift+f5` = `ai.revert` act only while the changes list has the keys (a new
  built-in list context) or the active editor tab is a diff tab; elsewhere they do nothing (in a
  focused session they go to the program).
- `ctrl+f8` / `ctrl+f9` keep resizing the terminal panel from the editor; they change the AI panel's
  width only while the AI panel has the keys. From the editor the AI width is a mouse drag of its
  separator only.
- `ctrl+f7` while the AI panel is shown still flips `terminal_position` for the session; flipped to
  right, the terminal stays at the bottom and an info notice says
  `Terminal goes to the right when the AI panel hides (f12)`.
- docs/keymap.md changes: the Global table gets the six rows above and `focus.editor` changes from
  "(palette only)" to `f3`; the `[terminal]` paragraph and the Built-in keys "Terminal" row say nine
  keys and add `f12`, `ctrl+f12`; the read-mode `tab` row reads "editor → explorer → terminal → AI
  panel (each when shown) → editor"; the `terminal.grow` / `terminal.shrink` rows add "(the AI
  panel's width while it has the keys)"; a new `### [ai]` section with the table, the lost-keys list
  and why there is no `ctrl+n` / `ctrl+w`; Built-in keys rows for the picker and the changes list;
  "Context tables: editor | explorer | palette | terminal" adds `ai`. README "Using vr-ide" gets an
  AI row (`f12` panel, `ctrl+f12` new session, `f6` changes); the welcome screen lists `f12  AI panel`.

## Tokens (new)

| Token | synthwave | Use | Reason |
|---|---|---|---|
| `mode_ai` | bg `#c77dff` (purple), fg editor bg, bold | `AI` / `DIFF` status badge | tells the panel's keys from `READ` (cyan), `EDIT` (orange), `TERM` (pink) |
| `attention` | `#ffe66d` (yellow), bold | `◆` mark, status badge bg | not the orange of modified `●` / warnings |
| `note` | `#9884c9` (the syntax comment tone), fitted to 4.5:1 | secondary text: list folders, picker notes, the D8 statement, inactive tab folders | the gutter `comment` (#8068b0, 3.9:1) is too faint for text; `dim` is too close to `fg` to rank below it |
| `diff_added_bg` | `#0e2c22` | added lines | every syntax colour keeps ≥ 4.5:1 on it (fg 13.2:1, comment 4.6:1, pink 6.6:1, `-` red 5.6:1) |
| `diff_removed_bg` | `#3a1027` | removed lines | same check (fg 14.5:1, comment 5.0:1, pink 7.2:1) |

Reused: `tab_bar`, `tab_active`, `tab_inactive`, `tab_warning`, `popup`, `popup_border`, `input`,
`list_selected`, `selection`, `git_added/modified/deleted`, `dim`, `focus`, `info`/`warning`/`error`,
terminal panel colours. Other themes derive the four new tokens with the existing `fit` floors.

## Components

| Component | Base | Props / variants | Behaviour |
|---|---|---|---|
| `ai_view` tab bar | `terminal_view` tab bar | label parts, `◆`, `x`, `+`, `±N`, `+N` | fit rule S1; click / drag like terminal tabs |
| `ai_view` grid | `terminal_view` grid | – | same renderer and selection/scroll keys |
| session picker | palette list rows | tool header / account / add / field rows | S2 |
| changes list | explorer rows | selected / unfocused-selected | S3 |
| diff tab | editor tab without file | M / A / D | read-only, find works |
| attention badge | status bar right part | 1 / several | S4 |
| completion list | palette list | folder / file / host rows, counter | S5 |

## Accessibility and authenticity checks

- Nothing by colour alone: `◆` attention, `M`/`A`/`D` letters, `+`/`-` signs, `x`, `±N`, `[exited 0]`.
- Contrast: all text tokens are the theme's fitted ones (≥ 4.5:1); new backgrounds checked above.
- Focus is always visible: cyan separator + status badge; selected rows use `list_selected`.
- Destructive actions (Revert, Remove account) confirm with Cancel as the default; Keep does not.
- No decoration that costs space: no boxes around the panel, one header row only where a list needs
  a title; copy is short and in the tools' own terms (`/login`, `--continue`, `CLAUDE_CONFIG_DIR`
  only in docs). Real content, no lorem; home is `/home/user`.

## Open questions (proposals; the default applies unless the user says otherwise — USER-6)

- Q1 An account belongs to the tool it was added under (claude's `work` is offered under claude only;
  its folder stays D11's `accounts/<name>/`, names unique across tools, the tool recorded beside it).
  Default: yes — one folder never serves as both `CLAUDE_CONFIG_DIR` and `CODEX_HOME`.
- Q2 Corrupt session file is kept as `session.toml.bad` before the next save. Default: yes.
- Q3 Remove account while sessions run on it: refused with a notice. Default: yes.

## Changelog

- 2026-11-27 DS round 1: S1–S6, key table, L1/L2 options; sessions per tool (amended D7) and
  per-session accounts (D11) included.
- 2026-11-27 DS fix round 1 (design QA): SEC-9/10 no-earlier-version and limit rows, SEC-13 banner,
  SEC-17 remove text, SEC-5 recent rows, SEC-18 restore refusals, SEC-12 revert line, account-busy
  notice, clipped copy, S4 wording, 120-col diff counts, `runs:` wrapping, 40x12, exited + empty list,
  key plumbing. New mockups: ai-changes-limits-120x40, ai-exited-empty-80x24, ai-account-busy-80x24,
  ai-restore-refused-80x24, palette-ssh-recent-80x24, ai-panel-40x12.
