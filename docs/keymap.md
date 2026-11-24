# Keymap

vr-ide ships VS Code's default Linux keybindings. The list below comes from
`crates/vr-config/defaults/keymap.toml` (keys) and `crates/vr-ide/src/commands/registry.rs`
(command names and titles). The palette (`ctrl+shift+p` or `F1`) lists every command with its key.

## Default keys

### Global (any focus, unless the focused panel binds the same key)

| Command | Title | Keys |
|---|---|---|
| `app.palette` | Command Palette | `ctrl+shift+p`, `f1` |
| `app.quit` | Quit | `ctrl+q` |
| `file.goto` | Go to File | `ctrl+p` |
| `file.open` | Open File | `ctrl+o` |
| `file.save` | Save | `ctrl+s` |
| `file.save_as` | Save As | `ctrl+shift+s` |
| `file.close` | Close File | `ctrl+w`, `ctrl+f4` |
| `explorer.new_file` | Explorer: New File | `ctrl+n` (creates the file on disk; there are no untitled buffers) |
| `tab.next` | Next Tab | `ctrl+pagedown` |
| `tab.prev` | Previous Tab | `ctrl+pageup` |
| `explorer.toggle` | Explorer: Toggle | `ctrl+b` |
| `explorer.toggle_focus` | Explorer: Toggle and Focus | `ctrl+e` (hidden: shows the explorer and focuses it; shown: hides it and the editor gets the keys) |
| `explorer.focus` | Explorer: Focus | `ctrl+shift+e` |
| `search.project` | Search in Project | `ctrl+shift+f` |
| `search.replace_project` | Replace in Project | `ctrl+shift+h` |
| `search.next_result` | Search: Next Result | `f4` |
| `search.prev_result` | Search: Previous Result | `shift+f4` |
| `focus.cycle` | Focus Next Part | `f6` |

### `[editor]`

A file opens in **read mode**: everything that does not change the text works (move, select, copy,
find, save, go to line, palette); typing and every text-changing key or command (enter, backspace,
delete, paste, cut, undo, redo, indent, line operations, replace in the file) does nothing and shows
the hint "Read mode: ctrl+i to edit". `ctrl+i` switches the tab to **edit mode**; `esc` returns to
read mode. A tab is back in read mode whenever the editor gets the keys on it again (tab switch,
back from the explorer, search panel, a dialog, or a palette closed without running a command).
The status bar shows `READ` or `EDIT`. Project replace and reloads from disk still update open
files in either mode.

| Command | Title | Keys |
|---|---|---|
| `edit.undo` | Undo | `ctrl+z` |
| `edit.redo` | Redo | `ctrl+shift+z`, `ctrl+y` |
| `edit.copy` | Copy | `ctrl+c` |
| `edit.cut` | Cut | `ctrl+x` |
| `edit.paste` | Paste | `ctrl+v` |
| `edit.select_all` | Select All | `ctrl+a` |
| `edit.delete_line` | Delete Line | `ctrl+shift+k` |
| `edit.duplicate_line` | Duplicate Line | `shift+alt+down`, `shift+alt+up` |
| `edit.move_line_up` | Move Line Up | `alt+up` |
| `edit.move_line_down` | Move Line Down | `alt+down` |
| `edit.indent` | Indent | `ctrl+]` |
| `edit.outdent` | Outdent | `ctrl+[`, `shift+tab` (most terminals send `ctrl+[` as Escape) |
| `edit.toggle_comment` | Toggle Line Comment | `ctrl+/` |
| `editor.edit_mode` | Enter Edit Mode | `ctrl+i` (without the kitty protocol the terminal sends it as `tab`; `tab` in read mode enters edit mode too) |
| `editor.read_mode` | Enter Read Mode (also clears the selection) | `escape` (in the find bar, palette, dialogs and panels `esc` keeps its own meaning) |
| `nav.goto_line` | Go to Line | `ctrl+g` |
| `find.open` | Find | `ctrl+f` |
| `find.replace_open` | Replace | `ctrl+h` |
| `find.next` | Find Next | `f3` |
| `find.prev` | Find Previous | `shift+f3` |
| `find.replace_one` | Replace One | `ctrl+shift+1` |
| `find.replace_all` | Replace All | `ctrl+alt+enter` |

### `[explorer]`

| Command | Title | Keys |
|---|---|---|
| `explorer.rename` | Explorer: Rename | `f2` |
| `explorer.delete` | Explorer: Delete | `delete` (asks first, then moves to the system trash) |
| `explorer.copy` | Explorer: Copy | `ctrl+c` |
| `explorer.cut` | Explorer: Cut | `ctrl+x` |
| `explorer.paste` | Explorer: Paste | `ctrl+v` |

### `[palette]`

No keymap entries; the palette's keys are built in (see below).

### `[terminal]`

| Command | Title | Keys |
|---|---|---|
| `focus.editor` | (M5, reserved) | `alt+q` |

A focused terminal (M5) receives every key except the ones in `[terminal]`. `alt+q` is reserved for
returning focus to the editor and is not a VS Code key.

### Commands without a default key (palette only)

| Command | Title |
|---|---|
| `file.reload` | Reload File from Disk |
| `file.save_overwrite` | Save (overwrite) |
| `explorer.reveal` | Explorer: Reveal Active File |
| `explorer.new_folder` | Explorer: New Folder |
| `explorer.move` | Explorer: Move |
| `explorer.toggle_ignored` | Explorer: Toggle Ignored Files |
| `explorer.toggle_hidden` | Explorer: Toggle Hidden Files |

Bind any of them in your own keymap, for example `"ctrl+alt+r" = "file.reload"`.

## Built-in keys (not in the keymap file)

Typing and cursor movement are built in and not listed in `keymap.toml`.

| Where | Keys |
|---|---|
| Editor | Arrows move; `home`/`end` go to line start/end; `ctrl+home`/`ctrl+end` go to document start/end; `pageup`/`pagedown` move one screen; `ctrl+left`/`ctrl+right` move by word; hold `shift` with any of these to extend the selection; `enter`, `tab`, `backspace`, `delete` and characters edit the text in edit mode; in read mode `tab` enters edit mode (it is what `ctrl+i` arrives as without the kitty protocol) |
| Everywhere | `esc` dismisses the oldest notice (when the palette, find bar or search panel is not focused; in the editor in edit mode `esc` first returns to read mode) |
| Explorer | `up`/`down`, `pageup`/`pagedown`, `home`/`end` move (with `shift`, extend the selection); `right` expands a folder or steps into it; `left` collapses or goes to the parent; `enter` opens the file or toggles the folder; `esc` returns focus to the editor. The explorer swallows plain typing |
| Palette | `up`/`down`, `pageup`/`pagedown` move; `enter` runs; `esc` closes |
| Dialogs | `left`/`right`/`tab`/`shift+tab` choose a button; `enter` confirms; `esc` cancels. A held (repeating) `enter` never confirms |
| Find bar | `enter` next match, `shift+enter` previous (in the replace field, `enter` replaces one); `tab`/`shift+tab` switch field; `alt+c` match case, `alt+w` whole word, `alt+r` regex; `esc` closes |
| Project search panel | `alt+c`/`alt+w`/`alt+r` toggle options; `tab`/`shift+tab` switch field; `enter` in the search field searches, in the replace field shows the replace preview, on a result opens it, and on a shown preview confirms the replace; `down` moves into the results; `up`/`down`/`pageup`/`pagedown`/`home`/`end` move in the list; `space` checks or unchecks a replace preview row; `esc` leaves the panel |

A user binding for a plain key in a context wins over that context's built-in key.

## Overriding keys

Create `~/.config/vr-ide/keymap.toml` (or `$VR_IDE_CONFIG_DIR/keymap.toml`). Only the keys you list
change; everything else keeps its default.

```toml
# Global key (works unless the focused panel binds the same key).
"ctrl+shift+d" = "edit.duplicate_line"

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
- Quotes around a key are optional when it is a plain word (`f2 = "explorer.rename"`).

An unknown command name is reported in a startup notice; the rest of the file still applies.
A terminal without the kitty keyboard protocol cannot send some chords (for example `ctrl+shift+p`);
vr-ide lists those in a startup notice, and `F1` opens the palette there. In such a terminal
`ctrl+shift+e` arrives as `ctrl+e` and toggles the explorer (a shown explorer hides); use `ctrl+e`
or `f6` to reach the tree.
