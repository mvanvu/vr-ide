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
