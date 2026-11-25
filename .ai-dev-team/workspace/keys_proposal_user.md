# User's key proposal (2026-11-25, step by step; not complete, not approved to build)

## Normal mode only
- space i      enter edit mode            (replaces ctrl+i and tab)
- space space  open the F1 command window

## Edit mode only
- letters/digits/symbols type text; enter new line; backspace / delete
- tab indent; shift+tab outdent            (ctrl+] removed)
- ctrl+z undo; ctrl+y redo; ctrl+x cut; ctrl+v paste
- ctrl+d delete line; ctrl+l duplicate line; ctrl+up / ctrl+down move line; ctrl+/ toggle comment
- ctrl+r replace in file, asks "one or all" (replaces f7 / shift+f7; user changed ctrl+h → ctrl+r)
- esc back to normal mode

## Still to come from the user
Both-modes section, explorer section.

## Leader facts noted to the user (no decision)
- ctrl+r is today "replace in project" (both modes); the user's both-modes section is still to come.

## Leader proposal for the both-modes section (asked by the user; awaiting the user's edits)
Changes from today: ctrl+r is contextual replace (editor → in file, edit mode; project search panel → replace
in project); ctrl+u, f7, shift+f7, ctrl+] removed; everything else unchanged. ctrl+f7 stays reserved (T19).
