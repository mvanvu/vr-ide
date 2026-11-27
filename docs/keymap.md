# Keymap

vr-ide has its own key set (USER-5, keyset v2 D261): one key per command, and every key is one
that a terminal without the kitty keyboard protocol can send and that GNOME Terminal and the GNOME
desktop leave alone: `ctrl` with a letter, or an F key alone, with `ctrl` or with `shift`. No
default key uses `alt` or `ctrl+shift`. The user approved two exceptions: the built-in `space`
prefix of read mode (below) and `ctrl+h` (byte 0x08; GNOME Terminal's Backspace sends 0x7f, so the
two stay apart). The list below comes from `crates/vr-config/defaults/keymap.toml` (keys) and
`crates/vr-ide/src/commands/registry.rs` (command names and titles). The palette (`f1`) lists
every command with its key.
Slash commands (type `/` in the palette) are listed in [commands.md](commands.md).

## Read mode and edit mode

A file opens in **read mode** (normal mode): everything that does not change the text works
(move, select, copy, find, save, go to line, palette); typing and every text-changing key or
command (enter, backspace, delete, paste, cut, undo, redo, indent, line operations, replace in the
file) does nothing and shows the hint "Read mode: space i to edit". A tab is back in read mode
whenever the editor gets the keys on it again (tab switch, back from the explorer or the find box, a
dialog, or a palette closed without running a command). The status bar shows `READ` or `EDIT`.
Project replace and reloads from disk still update open files in either mode.

| Mode | Key | Does |
|---|---|---|
| Read | `space` `i` | enter edit mode |
| Read | `space` `space` | open the command palette (like `f1`) |
| Read | `space` + any other key | shows the hint "space i: edit · space space: commands" and nothing else |
| Read | `tab` | focus the next part: editor → explorer (when shown) → terminal (when shown) → editor; the find box is not in the cycle (`ctrl+f` reaches it, it keeps `tab` for its fields) |
| Edit | characters, `enter`, `backspace`, `delete` | edit the text |
| Edit | `tab` / `shift+tab` | indent / outdent |
| Edit | `esc` | back to read mode |

A single `space` in read mode waits for the second key. The `space` keys are built in, not keymap
entries (a keymap binding is always one key). `ctrl+i` is not an edit key: without the kitty
protocol the terminal sends it as `tab`. From the explorer `tab` also moves the focus on; a focused
terminal keeps `tab` for its program (``ctrl+` `` hides the panel; a click on the editor gives the
keys back with the panel shown).

## Default keys

### Global (any focus, unless the focused panel binds the same key)

| Command | Title | Key |
|---|---|---|
| `app.palette` | Command Palette | `f1` |
| `app.quit` | Quit | `ctrl+q` |
| `file.goto` | Go to File | `ctrl+p` |
| `file.open` | Open File | `ctrl+o` |
| `file.save` | Save | `ctrl+s` |
| `file.close` | Close File | `ctrl+w` |
| `explorer.new_file` | Explorer: New File | `ctrl+n` (creates the file on disk; there are no untitled buffers; in the explorer see below) |
| `tab.next` | Next Tab | `f9` |
| `tab.prev` | Previous Tab | `shift+f9` |
| `explorer.toggle_focus` | Explorer: Toggle and Focus | `ctrl+e` (hidden: shows the explorer and focuses it; shown: hides it and the editor gets the keys) |
| `explorer.focus` | Explorer: Focus | `ctrl+b` |
| `explorer.reveal` | Explorer: Reveal Active File | `ctrl+k` |
| `explorer.toggle_hidden` | Explorer: Toggle Hidden Files | `ctrl+h` |
| `explorer.toggle_ignored` | Explorer: Toggle Ignored Files | `ctrl+f6` |
| `search.replace_project` | Replace in Project | `ctrl+r` (outside the editor, e.g. in the find box on the project; in the editor `ctrl+r` is the replace of the find box's scope) |
| `terminal.toggle` | Terminal: Toggle Terminal | ``ctrl+` `` (see below) |
| `terminal.hide` | Terminal: Hide Terminal | `shift+f12` |
| `terminal.next` | Terminal: Focus Next Terminal | `f8` |
| `terminal.prev` | Terminal: Focus Previous Terminal | `shift+f8` |
| `terminal.grow` | Terminal: Increase Panel Size | `ctrl+f8` (height at the bottom, width on the right) |
| `terminal.shrink` | Terminal: Decrease Panel Size | `ctrl+f9` (height at the bottom, width on the right) |
| `terminal.toggle_position` | Terminal: Toggle Panel Position | `ctrl+f7` (bottom ↔ right for this session; `terminal_position` in settings keeps it) |
| `editor.toggle_blame` | Toggle Blame Annotations | `shift+f2` (blame column on / off for this session, every editor tab, read and edit mode; see `docs/commands.md`) |
| `terminal.new` | Terminal: New Terminal | `ctrl+n` in a focused terminal only (`[terminal]`; not in the palette) |
| `terminal.close` | Terminal: Close Terminal | `ctrl+w` in a focused terminal only (`[terminal]`; not in the palette; asks first while a program runs) |
| `focus.editor` | Focus Editor | (palette only) |

On the right the panel takes the editor's full height under the tab bar and
`terminal_width_percent` (40 % by default, at least 20 columns) of the editor column; drag the
separator left of it to resize. While the editor column (the screen right of the explorer) is
under 60 columns wide the panel is shown at the bottom.

### `[editor]`

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
| `edit.outdent` | Outdent | `shift+tab` (`ctrl+[` is Escape for terminals) |
| `edit.toggle_comment` | Toggle Line Comment | `ctrl+/` |
| `editor.read_mode` | Enter Read Mode (also clears the selection) | `escape` (in the find box, palette, dialogs and panels `esc` keeps its own meaning) |
| `nav.goto_line` | Go to Line | `ctrl+g` |
| `find.open` | Find | `ctrl+f` (opens the find box on the file, on the project when no file is open; in the box it switches the scope File ↔ Project, see [The find box](#the-find-box)) |
| `find.next` | Find Next | `f4` (next match; in Project scope the next result) |
| `find.prev` | Find Previous | `shift+f4` (previous match or result) |
| `find.replace_open` | Replace | `ctrl+r` (the replace row of the box in its scope: File scope switches the tab to edit mode and `enter` in the replace field asks Replace: One / All / Cancel; Project scope, or no file open, is the project replace) |

### `[explorer]`

| Command | Title | Key |
|---|---|---|
| `explorer.rename` | Explorer: Rename | `f2` |
| `explorer.new` | Explorer: New File or Folder | `ctrl+n` (asks File / Folder, File first, then the name) |
| `explorer.delete` | Explorer: Delete | `delete` (asks first, then moves to the system trash) |
| `explorer.copy` | Explorer: Copy | `ctrl+c` |
| `explorer.cut` | Explorer: Cut | `ctrl+x` |
| `explorer.paste` | Explorer: Paste | `ctrl+v` |
| `explorer.search_folder` | Explorer: Search in Folder | (unbound; the right-click menu of a folder or root row) the find box in Project scope limited to the folder under the cursor |
| `explorer.replace_folder` | Explorer: Replace in Folder | (unbound; same menu) as above with the replace row |

### `[palette]`

No keymap entries; the palette's keys are built in (see below).

### `[terminal]`

A focused terminal receives every key except seven: ``ctrl+` ``, `f1` (the command window, from
the terminal too), `ctrl+f7` (panel position, bottom ↔ right), `ctrl+f8` / `ctrl+f9` (grow /
shrink the panel), `ctrl+n` (`terminal.new`: a new terminal tab, as the `+` button does) and
`ctrl+w` (`terminal.close`: closes this terminal, as its `x` does; it asks "Close terminal?" first
while a program other than the shell runs). Other global keys such as `ctrl+s`, `ctrl+p` or `f8` go to the program.
Add your own keys to `[terminal]` to take them from programs.

| Panel | ``ctrl+` `` does (wherever the keys are) |
|---|---|
| hidden | shows the panel and focuses it (a terminal starts if none runs) |
| shown | hides the panel; a focused terminal gives the keys to the editor; the terminals keep running |

To leave a terminal with the panel shown, click the editor. The panel's tab bar has a `+` after the
tabs (a new terminal in the active file's workspace folder) and an `x` at the end of each tab
(closes that terminal, its programs are hung up; the last one hides the panel; while a program other than the shell runs in it, "Close terminal?" asks first, Cancel being the default); in a focused terminal
`ctrl+n` and `ctrl+w` do the same. `shift+f12` (from the editor, explorer or find box) hides the panel too.
Without the kitty protocol the terminal sends ``ctrl+` `` as NUL, the same byte as
`ctrl+space`; vr-ide takes NUL as ``ctrl+` `` too, inside a focused terminal as well, unless
`terminal_pass_nul = true` (then the program gets NUL and a click on the editor gives the keys
back):

| Terminal | ``ctrl+` `` in a focused terminal | `ctrl+space` in a focused terminal |
|---|---|---|
| kitty protocol | hides the panel | goes to the program |
| legacy (default) | arrives as NUL and hides the panel | sends the same NUL: hides the panel |
| legacy, `terminal_pass_nul = true` | arrives as NUL and goes to the program | goes to the program |

A `ctrl+space` binding you add to `[terminal]` wins over the NUL rule.

### Commands without a default key (palette only)

| Command | Title |
|---|---|
| `file.save_as` | Save As |
| `file.reload` | Reload File from Disk |
| `file.save_overwrite` | Save (overwrite) |
| `search.project` | Search in Project (the find box in Project scope) |
| `search.next_result` | Search: Next Result |
| `search.prev_result` | Search: Previous Result |
| `find.replace_one` | Replace One |
| `find.replace_all` | Replace All |
| `explorer.new_folder` | Explorer: New Folder |
| `explorer.toggle` | Explorer: Toggle |
| `explorer.move` | Explorer: Move |
| `edit.indent` | Indent (`tab` in edit mode) |
| `focus.cycle` | Focus Next Part (`tab` in read mode) |
| `editor.edit_mode` | Enter Edit Mode (`space i` in read mode) |
| `workspace.add_folder` | Workspace: Add Folder… (opens `/ws add `) |
| `ssh.connect` | SSH: Connect to Host (opens `/ssh `: open sessions, recent targets, `~/.ssh/config` hosts) |
| `workspace.remove_folder` | Workspace: Remove Folder (the root row under the explorer cursor, else `/ws remove `; also on a root row's right-click menu as "Remove Folder from Workspace"; files on disk are never touched) |
| `workspace.save_as` | Workspace: Save As… (opens `/ws save `) |
| `workspace.open` | Workspace: Open… (opens `/ws open `) |
| `workspace.open_recent` | Workspace: Open Recent (opens `/ws recent `; the welcome screen lists them too, a click opens one) |

Match case and regex are part of the query (see [The find box](#the-find-box)); there are no
option commands. A new terminal starts in the active file's workspace folder (the first one still on disk
when that one is gone); there is no palette entry or slash command for it: the panel's `+`, or `ctrl+n` in a focused terminal.

## The find box

`ctrl+f` opens one box under the tab bar, with its results listed under it over the width of the
editor column. It has a scope, shown at its left: **File** (the default: the active file) or
**Project** (every workspace root). `ctrl+f` again while the box has the keys switches the scope;
the text goes along, and switching to Project with a query searches at once. With no file open the
box opens on the project.

| Query | Searches for |
|---|---|
| `foo` | the text `foo`, case-sensitive |
| `foo/i` | the text `foo`, ignoring case |
| `/fo+/` | the regular expression `fo+`, case-sensitive |
| `/fo+/i` | the regular expression `fo+`, ignoring case |
| `a\/b` | the text `a/b` (`\/` is a slash that is neither a regex delimiter nor the `/i` marker) |

A bad regular expression is shown in the box and nothing is searched. There is no whole-word
option (a regex `/\bfoo\b/` does it). A selection on one line becomes the query when the box opens
(slashes typed as `\/`, so it is searched literally).

**File scope** lists every match as `line  text` with the match highlighted, one row per match (a
line with two matches is listed twice), at most 20 000 (`20000+`). The list follows typing; the
editor shows the selected match and highlights the others while the box is open. **Project scope**
searches on `enter` and lists the results under the name of each file (and of each root when the
workspace has several roots), the status ("3 results in 2 files") on the box's first row.

The box keeps its first rows for the fields (before a terminal panel at the bottom takes its rows; beside a panel on the right the box sits over the editor: in a short
window the panel gets smaller, not the fields) and takes up to two fifths of the editor for the list
(at least 3 rows, fewer when there is less to list; a status or error note above the list comes on
top); the editor stays visible under it at every size.

## Built-in keys (not in the keymap file)

Typing and cursor movement are built in and not listed in `keymap.toml`.

| Where | Keys |
|---|---|
| Editor | Arrows move; `home`/`end` go to line start/end; `ctrl+home`/`ctrl+end` go to document start/end; `pageup`/`pagedown` move one screen; `ctrl+left`/`ctrl+right` move by word; hold `shift` with any of these to extend the selection; `enter`, `tab`, `backspace`, `delete` and characters edit the text in edit mode; read mode's `space` keys and `tab` are described above |
| Everywhere | `esc` dismisses the oldest notice (when the palette or the find box is not focused; in the editor in edit mode `esc` first returns to read mode) |
| Tab bar (editor) | A left click switches to the tab; a double left click (two presses on the same tab within 400 ms) also runs `explorer.reveal` for its file: the explorer is shown if hidden, expanded to the file (in its own workspace folder), the file selected and the explorer focused; a file the tree hides keeps the usual notice. Tabs without a file (`/keys`) only switch |
| Explorer | `up`/`down`, `pageup`/`pagedown`, `home`/`end` move (with `shift`, extend the selection); `right` expands a folder or steps into it; `left` collapses or goes to the parent; `enter` opens the file or toggles the folder; `tab` focuses the next part; `esc` returns focus to the editor. The explorer swallows plain typing |
| Explorer menu | A right click on a row (or below the rows, for the project folder) opens a menu of the `[explorer]` actions with their keys; dimmed ones do not apply there. `up`/`down`, `home`/`end` move over the others; `enter` or a left click runs one; `esc` or a click outside closes it; any other key closes it and does its usual job |
| Palette | `up`/`down`, `pageup`/`pagedown` move; `enter` runs; `esc` closes |
| Dialogs | `left`/`right`/`tab`/`shift+tab` choose a button; `enter` confirms; `esc` cancels. A held (repeating) `enter` never confirms |
| Find box, File scope | typing searches as you type; `up`/`down`/`pageup`/`pagedown` move in the list (the editor shows the match); `enter` in the find field goes to the match and gives the editor the keys in read mode (the box stays open: `f4`/`shift+f4` go on); `shift+enter` there selects the previous match and keeps the box (terminals with the kitty keyboard protocol; others send it as `enter`); in the replace field `enter` asks "Replace: One / All / Cancel" (One first; the box shows "enter replace"; each replacement is one undo step); `tab`/`shift+tab` switch field; `esc` closes the box and returns to read mode. A click on a row selects that match |
| Terminal | Every key goes to the program except ``ctrl+` ``, `f1`, `ctrl+f7`, `ctrl+f8`, `ctrl+f9`, `ctrl+n` (new terminal), `ctrl+w` (close this terminal) and these: `shift+pageup`/`shift+pagedown` scroll back (main screen only; a key sent to the program or a paste returns to the bottom, new output keeps the view); `ctrl+shift+v` pastes the clipboard (bracketed when the program asked for it). After the program exits, `enter` starts a new shell in the tab (an SSH tab connects to the same target again) and `ctrl+w` closes it. Mouse: drag selects and copies, the wheel scrolls back (programs that ask for the mouse get it; hold `shift` to select anyway), a click on a tab switches to it, dragging a tab along the bar moves it there (past the last tab shown, onto `+N`, it moves on one place per cell; at the bar's left edge, past the first tab shown, it moves toward the front the same way), on its `x` closes it (asks "Close terminal?" first while a program other than the shell runs in it; Cancel is the default), on the `+` opens a new terminal, dragging the rest of the tab bar (at the bottom) or the separator (on the right) resizes the panel |
| Find box, Project scope | `tab`/`shift+tab` switch field; `enter` in the search field searches, in the replace field shows the replace preview, on a result opens it (read mode; the box stays open), and on a shown preview confirms the replace; `down` moves into the results; `up`/`down`/`pageup`/`pagedown`/`home`/`end` move in the list; `space` checks or unchecks a replace preview row; `esc` closes a preview (cancels a running replace), otherwise closes the box and returns to read mode |

A user binding for a plain key in a context wins over that context's built-in key.

## Overriding keys

Create `~/.config/vr-ide/keymap.toml` (or `$VR_IDE_CONFIG_DIR/keymap.toml`). Only the keys you list
change; everything else keeps its default.

```toml
# Global key (works unless the focused panel binds the same key).
"f5" = "file.reload"
"ctrl+t" = "search.project"

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
  skipped (read mode's `space i` / `space space` are built in, not bindings).
- Quotes around a key are optional when it is a plain word (`f2 = "explorer.rename"`).

An unknown command name or an invalid key is reported in a startup notice; the rest of the file
still applies. The default keys all work in a terminal without the kitty protocol. If your own
keymap binds a key such a terminal cannot send (for example `ctrl+shift+p`), vr-ide lists it in a
startup notice (unless the command has another key that works), and `f1` opens the palette.
