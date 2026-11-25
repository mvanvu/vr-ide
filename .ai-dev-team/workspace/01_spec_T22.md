# Spec — T22 Unified find (ctrl+f)

flags: has_ui=true (terminal UI), security_sensitive=false, platforms=linux
source: user 2026-11-25 key list: "ctrl+f find in file/project (/…/ → regex, …/i → case-insensitive),
(result: list every line that matches, like PhpStorm)"; "ctrl+h → ctrl+r replace file/project"; leader
recommendation 5A approved (D261): one box, a scope switch in the box, default file; f3/shift+f3 next/previous.

## Behaviour
- ctrl+f opens one find box (both modes). Scope: **File** (default) or **Project**; pressing ctrl+f again while
  the box has focus switches the scope; the box shows the scope.
- Query syntax: `/pattern/` = regex; a trailing `/i` (`text/i` or `/pattern/i`) = case-insensitive; otherwise
  literal and case-sensitive. A literal slash is typed as `\/`. Invalid regex → inline error, no search.
- Results: a list under/next to the box with every matching line — `path:line  text` with the match
  highlighted (File scope: just `line  text`), grouped by file (and by root when more than one workspace
  root). Up/Down move in the list; Enter jumps to the match (focus to the editor in read mode); Esc closes
  the box and returns to read mode. Matches in the editor stay highlighted while the box is open.
- f3 / shift+f3 go to the next / previous result from the editor (shift+f3 limitation D264 stays open).
- Typing updates File-scope results live; Project scope searches on Enter (as today) on the existing worker.
- ctrl+r opens the same box with a replace field: File scope → replace in the file (edit mode; Enter one,
  ctrl+a all — D265 open); Project scope → the existing preview + confirm flow.
- The old find bar toggles (match case / whole word / regex) are replaced by the syntax; whole word has no
  syntax (not asked).
- Large results: File scope keeps the D124 cap; Project scope keeps vr-search limits; the list is virtual.

## Acceptance criteria
- AC-22a: ctrl+f in a file lists every matching line; Enter jumps; Esc back to read mode.
- AC-22b: ctrl+f again switches to Project scope; results grouped by file/root; Enter opens the file at the match.
- AC-22c: `/fo+/` searches as regex, `foo/i` and `/fo+/i` ignore case, `\/` is a literal slash, bad regex errors.
- AC-22d: f3 / shift+f3 step through results; ctrl+r replace works in both scopes.
- AC-22e: snapshots 80x24, 120x40, 40x12; 1M-line file and 50k-file project stay responsive.
