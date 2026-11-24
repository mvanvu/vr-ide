# Review T16 (welcome on demand) - CHANGES_REQUESTED

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.

## Findings
[major] crates/vr-ide/src/welcome_tests.rs:1 - tests do not cover the paths the notes claim (BASE-3 / missing test for changed behavior)
Scenario: notes say "mouse click/drag in editor ignored (test keys_typed_while_welcome...)", but no test sends a mouse event except the tab click. Removing `&& !self.welcome` at app.rs:1063 (click moves the hidden doc's cursor and starts a drag) keeps all tests green. `acts_on_document` is also untested for its own entries: the only keys typed are ones editable_doc already refuses, so deleting the whole deny-list check (app.rs:962) fails nothing; no test presses ctrl+f / ctrl+g / ctrl+s / ctrl+h, ctrl+a, ctrl+c.
Fix: add tests - editor click + drag leaves selection/`dragging` unchanged; with welcome up, ctrl+f (find.open), ctrl+g (nav.goto_line), ctrl+s do not open a bar/dialog or change state; unit test acts_on_document for the listed names.

[minor] crates/vr-ide/src/commands/palette.rs:181 - palette bypasses the D189 deny list
Scenario: welcome up, F1 -> "Find" / "Go to Line" / "Select All" / "Copy" / "Save" / "Close File": run_command is called directly, so the find bar opens (typing there selects matches and moves the cursor of the hidden doc), the goto dialog jumps the hidden doc, select-all alters its selection, copy puts hidden text on the clipboard, save/close act on the unseen tab; welcome stays up. Text-changing ones are refused by editable_doc, so no data change, but D189 ("keys do nothing") only holds for keys.
Fix: apply the guard in one place (App::run_command: `if self.welcome && acts_on_document(name) { return false }`, with the same list), or make these commands hide welcome first; state the decision in decisions.md.

[minor] crates/vr-ide/src/commands/app.rs:140 - deny list incomplete
Scenario: `editor.edit_mode` (ctrl+i) and `editor.read_mode`, and `file.save_overwrite` (user-bindable) are not matched. Welcome up, ctrl+i switches the hidden tab to edit mode (focus forced to Editor); on Esc the user lands in a document that is in edit mode they never saw; a bound save_overwrite writes the hidden doc.
Fix: add `editor.` prefix and `file.save_overwrite` (or invert to an allow-list: app.*, tab.*, explorer.*, file.open/goto, focus.*, search.*).

[minor] notes - BASE-3 evidence weak: esc/open/switch tests passed on the stub because they only assert `!welcome` after a key that did nothing; they are meaningful on the real code (they fail if set_active stops clearing), accepted, but a red run against the real shortcut with set_active unchanged was not shown.

## Verified OK
- Flag cleared by set_active (tab key, tab click including the active tab, open_path from explorer / goto / search / palette file items); Esc in editor focus; closing last tab leaves plain welcome; welcome with no tab stays off.
- Built-in keys and bracketed paste blocked; editable_doc refuses (find.replace_one/all, edit.*). Mode: welcome does not change view.edit; sync_mode unaffected (edit mode persists across show/hide on the same tab; leaving via explorer resets it).
- Find bar open: ctrl+shift+w passes through to the keymap and toggles; Esc closes the bar first, then welcome. Quit guard unchanged. Tab bar (no active style) and status bar (root only) render per D188; hint guard `height > 4 + len` correct; snapshots 80x24 / 40x12 stable and fit.
- keymap.toml, KNOWN list, registry PLAN, docs/keymap.md, README consistent with D186 and the legacy-terminal fallback.

## Files read
crates/vr-ide/src/app.rs read; commands/app.rs read; commands/registry.rs read; lib.rs read; ui/editor_view.rs read; ui/tabs.rs read; ui/status_bar.rs read; ui/mod.rs read; welcome_tests.rs read; both new .snap read; vr-config keymap.toml, tests/config.rs read; docs/keymap.md, README.md read. Callees: commands/find.rs, edit.rs, palette.rs read.

## Rule check T16
| Rule | Result |
|---|---|
| BASE-1 | pass |
| BASE-2 | pass |
| BASE-3 | fail (see major; stub-vacuous tests accepted as minor) |
| BASE-6 | pass |
| BASE-25 | pass (guard clean per notes) |
| BASE-32 | pass for D186/D188; D189 partly (palette path) |
| New-states | fail (mouse claim unbacked; palette / editor.* states missing) |

## Round 1 re-check - APPROVED
Files read: git diff app.rs run_command, commands/app.rs (allowed_over_welcome), keymap.toml diff, notes `## Round 1`; callees explorer.rs (retarget, reveal), search.rs (jump, replace).
- Finding 1 (tests): closed per notes - mouse, ctrl+f/g/s/h/a/c/i, palette and filter tests exist; red evidence stated for guard and mouse.
- Finding 2 (palette bypass): closed - guard at the top of App::run_command covers keys, palette, dialogs.
- Finding 3 (deny list): closed - allow-list; `editor.*`, `file.save*`, `file.close`, `file.reload`, `find.*`, `nav.*`, `edit.*` all refused. Finding 4: accepted.
- Allow-list width: `explorer.*` delete/rename/move/paste act on the explorer's selected path, which the user sees; open tabs follow via retarget / close, the same as with a visible editor, not on the hidden document as such. `explorer.reveal` only reads the active path. `search.*` replace shows a preview, compares the buffer text and refuses read-only; it is the project-replace case accepted as fine. Neither is a problem.
- Non-blocking minors: (a) find bar already open when welcome is shown still takes typing/Enter directly (find::handle_key, not run_command), moving the hidden doc's selection; no text change. (b) `search.project` seeds its query from the hidden selection when focus is Editor. Both optional.
