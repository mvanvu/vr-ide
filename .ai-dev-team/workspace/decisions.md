# Decisions taken while the user is away

(none yet — entries are numbered D<n> continuing after the spec's D13; agents append `D? (T{id})`)
D14 (T1) clippy.toml optional — skipped — no lint config needed yet
D15 (T7) Should .gitignore rules apply when the project is not a git repo? — yes (require_git off in tree and file index; same policy for T9 search recommended) — users expect node_modules/target hidden either way; one policy everywhere
D16 (T7) Dotfiles in go-to-file? — included (only `.git` and gitignored excluded); the tree still hides them unless show_hidden — VS Code quick-open behaviour; `.env`, `.github/*` must be reachable
D17 (T7) Symlinks — listed as `Symlink(File|Dir|Loop|Broken)`; a link to its own dir or an ancestor is `Loop` and not expandable; copy keeps links as links; file index never follows links — no infinite recursion
D18 (T7) Huge non-ignored trees in the file index — capped at 200 000 files, `FileIndex::truncated()` reports it — bounded memory; UI can show a notice
D19 (T3) Keymap TOML needs quoted keys (`"ctrl+s"`), but AC-5 writes `ctrl+s = "file.save"` — loader quotes such bare keys before parsing (last `=` before the value is the separator) — AC-5's literal example works; canonical quoted form still valid
D20 (T3) Key contexts — `[editor]`/`[explorer]`/`[palette]`/`[terminal]` tables or `{ command, when }` inline form; lookup = context then global, terminal has no global fallback (D8); `command_for(k)` = editor focus — VS Code `when` equivalent, kept minimal
D21 (T3) ctrl+n (VS Code: new untitled file) — bound to `explorer.new_file` — no untitled buffers in the command list
D22 (T3) Commands with no VS Code default key (file.reload, explorer.reveal, explorer.new_folder, explorer.move, explorer.toggle_ignored, explorer.toggle_hidden) — left unbound, palette only — D10 says follow VS Code defaults; f6 → focus.cycle (VS Code focusNextPart), f4/shift+f4 → search next/prev result, shift+tab also → edit.outdent (terminals send ctrl+[ as Esc)
D23 (T3) Settings defaults — tab_width 4, insert_spaces true, theme "dark", show_hidden true (VS Code shows dotfiles), show_ignored false, max_highlight_bytes 10 MiB, scrollback 10000 (C1) — bad value/unknown key → warning, default kept per key
D24 (T7, leader) Cross-device move (EXDEV) in vr-fs — return a `CrossDevice` error, never copy+delete — the narrowest choice; nothing is deleted or half-copied; the UI can offer copy instead. confirm-on-return
D25 (T7, leader) No-overwrite guarantee — approve adding `rustix` (fs feature) to vr-fs for `renameat2(RENAME_NOREPLACE)`; files copied with `File::create_new`; remaining check-then-act windows (directory copy) documented in code — atomic refusal protects user files written by AI agents under the editor
D26 (T2) how T4 derives "modified" when edit + undo must read as unmodified (a monotonic `version` cannot return) — add `Buffer::revision()` (id of the top undo group, 0 = initial; equal value = equal text) next to `version()`; T4 stores the revision at save — `version` stays monotonic as the plan says.
D27 (T2) indent width for `indent`/`outdent` (contract lists them without params) — `BufferOptions { tab_width, indent_unit }` on the Buffer (default 4 / four spaces), set by the app from settings — keeps the contract signatures.
D28 (T2) undo grouping — single typed graphemes (no line break) coalesce; same-kind backspace/delete runs coalesce; any cursor move, undo/redo, `break_undo_group`, a paste or Enter, or 1 s idle (injectable `Clock`) starts a new group — VS Code-like and AC-3 testable.
D29 (T2) Home key — smart home (first non-blank, then column 0); up/down keep a display-column goal; past first/last line jump to doc start/end — VS Code defaults (D10).
D30 (T2) toggle_comment — add `token ` at the smallest indent of non-blank lines unless all are commented, then remove token + one space; blank lines untouched — VS Code behaviour, keeps indentation.
D31 (T2, leader) Line breaks — only \n, \r\n and \r count as line breaks (ropey unicode_lines off) — matches VS Code, git, grep -n and php line numbers
