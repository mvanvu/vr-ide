# Keymap

vr-ide has its own key set (USER-5, D250): one key per command, and every key is one that a
terminal without the kitty keyboard protocol can send and that GNOME Terminal and the GNOME desktop
leave alone: `ctrl` with a letter, or an F key alone, with `ctrl` or with `shift`. No default key
uses `alt` or `ctrl+shift`, and there are no key sequences. The list below comes from
`crates/vr-config/defaults/keymap.toml` (keys) and `crates/vr-ide/src/commands/registry.rs`
(command names and titles). The palette (`f1`) lists every command with its key.

## Default keys

### Global (any focus, unless the focused panel binds the same key)

| Command | Title | Key |
|---|---|---|
| `app.palette` | Command Palette | `f1` |
| `app.quit` | Quit | `ctrl+q` |
| `file.goto` | Go to File | `ctrl+p` |
| `file.open` | Open File | `ctrl+o` |
| `file.save` | Save | `ctrl+s` |
| `file.save_as` | Save As | `f12` |
| `file.close` | Close File | `ctrl+w` |
| `file.reload` | Reload File from Disk | `f5` |
| `explorer.new_file` | Explorer: New File | `ctrl+n` (creates the file on disk; there are no untitled buffers) |
| `tab.next` | Next Tab | `f9` |
| `tab.prev` | Previous Tab | `shift+f9` |
| `explorer.toggle_focus` | Explorer: Toggle and Focus | `ctrl+e` (hidden: shows the explorer and focuses it; shown: hides it and the editor gets the keys) |
| `explorer.focus` | Explorer: Focus | `ctrl+b` |
| `explorer.reveal` | Explorer: Reveal Active File | `ctrl+k` |
| `explorer.toggle_hidden` | Explorer: Toggle Hidden Files | `ctrl+f5` |
| `explorer.toggle_ignored` | Explorer: Toggle Ignored Files | `ctrl+f6` |
| `search.project` | Search in Project | `ctrl+t` |
| `search.replace_project` | Replace in Project | `ctrl+r` |
| `search.next_result` | Search: Next Result | `f4` |
| `search.prev_result` | Search: Previous Result | `shift+f4` |
| `find.toggle_case` | Find: Toggle Match Case | `ctrl+f1` |
| `find.toggle_word` | Find: Toggle Whole Word | `ctrl+f2` |
| `find.toggle_regex` | Find: Toggle Regular Expression | `ctrl+f3` |
| `focus.cycle` | Focus Next Part | `f6` |
| `terminal.toggle` | Terminal: Toggle Terminal | ``ctrl+` `` (see below) |
| `terminal.new` | Terminal: New Terminal | `ctrl+f12` |
| `terminal.hide` | Terminal: Hide Terminal | `shift+f12` |
| `terminal.close` | Terminal: Close Terminal | `ctrl+f4` |
| `terminal.next` | Terminal: Focus Next Terminal | `f8` |
| `terminal.prev` | Terminal: Focus Previous Terminal | `shift+f8` |
| `terminal.grow` | Terminal: Increase Panel Height | `ctrl+f8` |
| `terminal.shrink` | Terminal: Decrease Panel Height | `ctrl+f9` |

The find options (`ctrl+f1` / `ctrl+f2` / `ctrl+f3`) act on the find bar while it is open, wherever
the keys are; without a find bar they act on the project search panel while it is shown; with
neither, a notice names the key that opens Find. In the explorer `ctrl+f2` is New Folder (the
explorer's own key wins there). `ctrl+f7` is kept free for the terminal panel position command.

### `[editor]`

A file opens in **read mode**: everything that does not change the text works (move, select, copy,
find, save, go to line, palette); typing and every text-changing key or command (enter, backspace,
delete, paste, cut, undo, redo, indent, line operations, replace in the file) does nothing and shows
the hint "Read mode: ctrl+i to edit". `ctrl+i` switches the tab to **edit mode**; `esc` returns to
read mode. A tab is back in read mode whenever the editor gets the keys on it again (tab switch,
back from the explorer, search panel, a dialog, or a palette closed without running a command).
The status bar shows `READ` or `EDIT`. Project replace and reloads from disk still update open
files in either mode.

| Command | Title | Key |
|---|---|---|
| `edit.undo` | Undo | `ctrl+z` |
| `edit.redo` | Redo | `ctrl+y` |
| `edit.copy` | Copy | `ctrl+c` |
| `edit.cut` | Cut | `ctrl+x` |
| `edit.paste` | Paste | `ctrl+v` |
| `edit.select_all` | Select All | `ctrl+a` |
| `edit.delete_line` | Delete Line | `ctrl+d` |
| `edit.duplicate_line` | Duplicate Line | `ctrl+l` |
| `edit.move_line_up` | Move Line Up | `ctrl+up` |
| `edit.move_line_down` | Move Line Down | `ctrl+down` |
| `edit.indent` | Indent | `ctrl+]` |
| `edit.outdent` | Outdent | `shift+tab` (`ctrl+[` is Escape for terminals) |
| `edit.toggle_comment` | Toggle Line Comment | `ctrl+/` |
| `editor.edit_mode` | Enter Edit Mode | `ctrl+i` (without the kitty protocol the terminal sends it as `tab`; `tab` in read mode enters edit mode too) |
| `editor.read_mode` | Enter Read Mode (also clears the selection) | `escape` (in the find bar, palette, dialogs and panels `esc` keeps its own meaning) |
| `nav.goto_line` | Go to Line | `ctrl+g` |
| `find.open` | Find | `ctrl+f` |
| `find.replace_open` | Replace | `ctrl+u` |
| `find.next` | Find Next | `f3` |
| `find.prev` | Find Previous | `shift+f3` |
| `find.replace_one` | Replace One | `f7` |
| `find.replace_all` | Replace All | `shift+f7` |

### `[explorer]`

| Command | Title | Key |
|---|---|---|
| `explorer.rename` | Explorer: Rename | `f2` |
| `explorer.new_folder` | Explorer: New Folder | `ctrl+f2` |
| `explorer.delete` | Explorer: Delete | `delete` (asks first, then moves to the system trash) |
| `explorer.copy` | Explorer: Copy | `ctrl+c` |
| `explorer.cut` | Explorer: Cut | `ctrl+x` |
| `explorer.paste` | Explorer: Paste | `ctrl+v` |

### `[palette]`

No keymap entries; the palette's keys are built in (see below).

### `[terminal]`

A focused terminal receives every key except one: ``ctrl+` ``. Global keys such as `ctrl+s`,
`ctrl+w` or `ctrl+p` go to the program. Add your own keys to `[terminal]` to take them from
programs.

| Where | ``ctrl+` `` does |
|---|---|
| Editor, panel hidden | shows the panel and focuses it (a terminal starts if none runs) |
| Editor, panel shown | focuses the panel |
| Focused terminal | gives the keys back to the editor; the panel stays shown |
| Focused terminal, window so short that the panel covers the whole editor | hides the panel (there is no editor row to give the keys to); the panel's hint then reads ``ctrl+` hide`` |

`shift+f12` (from the editor, explorer, find bar or search panel) hides the panel; the terminals
keep running. Without the kitty protocol the terminal sends ``ctrl+` `` as NUL, the same byte as
`ctrl+space`; vr-ide takes NUL as ``ctrl+` `` too, inside a focused terminal as well, unless
`terminal_pass_nul = true` (then the program gets NUL and a click on the editor gives the keys
back):

| Terminal | ``ctrl+` `` in a focused terminal | `ctrl+space` in a focused terminal |
|---|---|---|
| kitty protocol | gives the keys back | goes to the program |
| legacy (default) | arrives as NUL and gives the keys back | sends the same NUL: gives the keys back |
| legacy, `terminal_pass_nul = true` | arrives as NUL and goes to the program | goes to the program |

A `ctrl+space` binding you add to `[terminal]` wins over the NUL rule.

### Commands without a default key (palette only)

| Command | Title |
|---|---|
| `file.save_overwrite` | Save (overwrite) |
| `explorer.toggle` | Explorer: Toggle |
| `explorer.move` | Explorer: Move |

The workspace commands, when they come, are palette only too.

## Built-in keys (not in the keymap file)

Typing and cursor movement are built in and not listed in `keymap.toml`.

| Where | Keys |
|---|---|
| Editor | Arrows move; `home`/`end` go to line start/end; `ctrl+home`/`ctrl+end` go to document start/end; `pageup`/`pagedown` move one screen; `ctrl+left`/`ctrl+right` move by word; hold `shift` with any of these to extend the selection; `enter`, `tab`, `backspace`, `delete` and characters edit the text in edit mode; in read mode `tab` enters edit mode (it is what `ctrl+i` arrives as without the kitty protocol) |
| Everywhere | `esc` dismisses the oldest notice (when the palette, find bar or search panel is not focused; in the editor in edit mode `esc` first returns to read mode) |
| Explorer | `up`/`down`, `pageup`/`pagedown`, `home`/`end` move (with `shift`, extend the selection); `right` expands a folder or steps into it; `left` collapses or goes to the parent; `enter` opens the file or toggles the folder; `esc` returns focus to the editor. The explorer swallows plain typing |
| Palette | `up`/`down`, `pageup`/`pagedown` move; `enter` runs; `esc` closes |
| Dialogs | `left`/`right`/`tab`/`shift+tab` choose a button; `enter` confirms; `esc` cancels. A held (repeating) `enter` never confirms |
| Find bar | `enter` next match, `shift+enter` previous (in the replace field, `enter` replaces one); `tab`/`shift+tab` switch field; `esc` closes |
| Terminal | Every key goes to the program except ``ctrl+` `` and these: `shift+pageup`/`shift+pagedown` scroll back (main screen only; any key or new output returns to the bottom); `ctrl+shift+v` pastes the clipboard (bracketed when the program asked for it). After the program exits, `enter` starts a new shell in the tab and the Close File key (`ctrl+w`) closes it. Mouse: drag selects and copies, the wheel scrolls back (programs that ask for the mouse get it; hold `shift` to select anyway), a click on a tab switches to it, dragging the tab bar resizes the panel |
| Project search panel | `tab`/`shift+tab` switch field; `enter` in the search field searches, in the replace field shows the replace preview, on a result opens it, and on a shown preview confirms the replace; `down` moves into the results; `up`/`down`/`pageup`/`pagedown`/`home`/`end` move in the list; `space` checks or unchecks a replace preview row; `esc` leaves the panel |

A user binding for a plain key in a context wins over that context's built-in key.

## Overriding keys

Create `~/.config/vr-ide/keymap.toml` (or `$VR_IDE_CONFIG_DIR/keymap.toml`). Only the keys you list
change; everything else keeps its default.

```toml
# Global key (works unless the focused panel binds the same key).
"f2" = "file.reload"

# Remove a built-in binding: an empty command name.
"ctrl+q" = ""

# Context tables: editor | explorer | palette | terminal.
[editor]
"ctrl+k" = "edit.delete_line"

[explorer]
"f5" = "explorer.reveal"
```

A line can also name its context inline at the top level:
`"f2" = { command = "explorer.rename", when = "explorer" }`.

Lookup order: the focused context's table first, then the global keys. The terminal context is the
exception: a focused terminal gets every key except the ones in `[terminal]`.

Key spelling:

- Modifiers `ctrl`, `shift`, `alt` in any order, case-insensitive, joined with `+` to one key.
- The key is a letter, digit or symbol (`"ctrl+/"`, `"ctrl+]"`), `f1`..`f24`, `enter`,
  `escape`, `tab`, `backspace`, `delete`, `insert`, `home`, `end`, `pageup`, `pagedown`, `up`,
  `down`, `left`, `right`, `space`, or `plus`.
- The plus key is spelled `plus`, because `+` separates the parts: `"ctrl+plus" = "..."`.
- A binding is one key; a key with a space in it (`"ctrl+x s"`) is reported as an invalid key and
  skipped.
- Quotes around a key are optional when it is a plain word (`f2 = "explorer.rename"`).

An unknown command name or an invalid key is reported in a startup notice; the rest of the file
still applies. The default keys all work in a terminal without the kitty protocol. If your own
keymap binds a key such a terminal cannot send (for example `ctrl+shift+p`), vr-ide lists it in a
startup notice (unless the command has another key that works), and `f1` opens the palette.
