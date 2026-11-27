# Spec — T21 Keyset v2 (user's own list, approved 2026-11-25 "follow your recommendations")

flags: has_ui=true (terminal UI), security_sensitive=false, platforms=linux
source: user's lists (keys_proposal_user.md) + leader recommendations 1–7 approved (D261).

## Normal mode only (editor in read mode)
| Key | Command |
|---|---|
| `space i` | enter edit mode (replaces ctrl+i and tab) |
| `space space` | open the F1 command window (palette) |
| `tab` | cycle focus (editor → explorer → terminal …) |

## Edit mode only
| Key | Command |
|---|---|
| characters, enter, backspace, delete | edit text |
| `tab` / `shift+tab` | indent / outdent |
| `ctrl+z` / `ctrl+y` | undo / redo |
| `ctrl+x` / `ctrl+v` | cut / paste |
| `ctrl+d` | delete line |
| `ctrl+l` | duplicate line |
| `ctrl+up` / `ctrl+down` | move line up / down |
| `ctrl+/` | toggle comment |
| `esc` | back to normal mode |

## Both modes
| Key | Command |
|---|---|
| arrows, home/end, ctrl+home/end, pageup/pagedown, ctrl+left/right, shift+move | move / select (built in) |
| `ctrl+a` | select all |
| `ctrl+c` | copy |
| `ctrl+f` | find (T22: one box for file and project; result list; ctrl+f again toggles scope) |
| `f3` / `shift+f3` | next / previous result |
| `ctrl+r` | replace: in the editor → in the file (edit mode, asks one / all); in the project search → in the project |
| `ctrl+s` | save |
| (removed) | save as → `/save-as` (D263) |
| (removed) | reload → `/reload` (D263) |
| `ctrl+o` | open file |
| `ctrl+p` | go to file |
| `ctrl+n` | new file |
| `ctrl+w` | close tab |
| `ctrl+g` | go to line |
| `f9` / `shift+f9` | next / previous tab |
| `ctrl+e` | toggle explorer |
| `ctrl+b` | focus explorer |
| `ctrl+k` | reveal the active file in the explorer |
| `ctrl+h` | explorer: show hidden files |
| `ctrl+f6` | explorer: show ignored files |
| ``ctrl+` `` | toggle terminal |
| `ctrl+f12` | new terminal |
| `shift+f12` | hide terminal |
| `ctrl+f4` | close terminal |
| `f8` / `shift+f8` | next / previous terminal |
| `ctrl+f8` / `ctrl+f9` | terminal panel taller / shorter |
| `ctrl+f7` | terminal position bottom / right (reserved for T19) |
| `f1` | command window (palette) |
| `ctrl+q` | quit |

## Explorer focus
| Key | Command |
|---|---|
| `f2` | rename |
| `ctrl+n` | new file or folder: a small choice "File / Folder" then the name prompt (user 2026-11-25) |
| `delete` | delete (to trash) |
| `ctrl+c` / `ctrl+x` / `ctrl+v` | copy / cut / paste a file or folder (folders recursively, as vr-fs already does) |

## Removed
ctrl+i (edit mode; in legacy terminals it is tab, which now cycles focus in normal mode), ctrl+], ctrl+u, f7,
shift+f7, ctrl+t, ctrl+f2 (new folder is under ctrl+n in the explorer), f4/shift+f4, f6, ctrl+f1/f2/f3, ctrl+f5. Commands without a key stay in the palette.

## Rules
- `space` sequences are built in for normal mode only (not keymap sequences; user keymaps still reject
  sequences). A single space in normal mode does nothing until the second key; `space` + other key → hint.
- USER-5 exceptions approved by the user: `space` prefix in normal mode, `ctrl+h` (0x08; GNOME Terminal's
  Backspace sends 0x7F).
- Every key still reaches vr-ide from GNOME Terminal without the kitty protocol (test).
- docs/keymap.md, README, welcome hints, palette and read-mode hint ("Read mode: space i to edit") updated.

## Acceptance criteria
- AC-21a: each key above runs its command, with and without the kitty protocol (dispatch test + pty walk).
- AC-21b: normal-mode `space i`, `space space`, `tab`; edit-mode `tab` indents; `esc` back to normal.
- AC-21c: `ctrl+r` opens in-file replace (asks one / all) in the editor and project replace in the search panel.
- AC-21d: removed keys do nothing (or are free); docs match keymap (existing guard test).
