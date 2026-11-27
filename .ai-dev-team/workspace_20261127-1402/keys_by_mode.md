# vr-ide keys by mode (draft for the user to edit)

Source: crates/vr-config/defaults/keymap.toml at master abdbe42, plus built-in editor keys.
Edit freely: change a key, move a row to another section, delete or add rows. The leader reads this file
back and turns your changes into a proposal.

## Normal mode only (read mode)

| Key | Command |
|---|---|
| `ctrl+i` | enter edit mode |
| `tab` | enter edit mode (legacy terminals send ctrl+i as tab) |
| `space space` | open the F1 command window (proposed D260, not built) |

Unbound in normal mode today: letters, digits, punctuation and a single `space`.

## Edit mode only
In normal mode these show the hint "Read mode: ctrl+i to edit".

| Key | Command |
|---|---|
| letters, digits, symbols | type text |
| `enter` | new line |
| `backspace` | delete before the cursor |
| `delete` | delete after the cursor |
| `tab` | indent |
| `ctrl+]` | indent |
| `shift+tab` | outdent |
| `ctrl+z` | undo |
| `ctrl+y` | redo |
| `ctrl+x` | cut |
| `ctrl+v` | paste |
| `ctrl+d` | delete line |
| `ctrl+l` | duplicate line |
| `ctrl+up` | move line up |
| `ctrl+down` | move line down |
| `ctrl+/` | toggle comment |
| `f7` | replace one (in file) |
| `shift+f7` | replace all (in file) |
| `esc` | back to normal mode |

## Both modes

| Key | Command |
|---|---|
| arrows | move |
| `home` / `end` | line start / end |
| `ctrl+home` / `ctrl+end` | document start / end |
| `pageup` / `pagedown` | one screen up / down |
| `ctrl+left` / `ctrl+right` | word left / right |
| `shift` + any move key | extend the selection |
| `ctrl+a` | select all |
| `ctrl+c` | copy |
| `ctrl+f` | find in file |
| `f3` / `shift+f3` | next / previous match |
| `ctrl+u` | open the replace field |
| `ctrl+f1` | find: match case |
| `ctrl+f2` | find: whole word |
| `ctrl+f3` | find: regex |
| `ctrl+t` | search in project |
| `ctrl+r` | replace in project |
| `f4` / `shift+f4` | next / previous search result |
| `ctrl+s` | save |
| `f12` | save as |
| `f5` | reload from disk |
| `ctrl+o` | open file |
| `ctrl+p` | go to file |
| `ctrl+n` | new file |
| `ctrl+w` | close tab |
| `ctrl+g` | go to line |
| `f9` / `shift+f9` | next / previous tab |
| `f6` | cycle focus |
| `ctrl+e` | toggle explorer |
| `ctrl+b` | focus explorer |
| `ctrl+k` | reveal the active file in the explorer |
| `ctrl+f5` | explorer: show hidden files |
| `ctrl+f6` | explorer: show ignored files |
| `` ctrl+` `` | toggle terminal |
| `ctrl+f12` | new terminal |
| `shift+f12` | hide terminal |
| `ctrl+f4` | close terminal |
| `f8` / `shift+f8` | next / previous terminal |
| `ctrl+f8` / `ctrl+f9` | terminal panel taller / shorter |
| `f1` | palette (command window) |
| `ctrl+q` | quit |

## Explorer focus

| Key | Command |
|---|---|
| `f2` | rename |
| `ctrl+f2` | new folder |
| `delete` | delete (to trash) |
| `ctrl+c` | copy file |
| `ctrl+x` | cut file |
| `ctrl+v` | paste file |
