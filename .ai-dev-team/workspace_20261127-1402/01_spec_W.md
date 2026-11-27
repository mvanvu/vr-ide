# Spec — Multi-root workspace

flags: has_ui=true (terminal UI), security_sensitive=false, platforms=linux
source: user 2026-11-25 "open several repo folders (manage multiple projects)"; picked "Workspace nhiều thư mục" (VS Code-style multi-root) in the question at 05:50.

## Goal
One vr-ide window holds several project folders (roots). The explorer, go to file, project search
and replace, file watching and the terminal work across all roots.

## Scope
- Start: `vr-ide dir1 dir2 …` opens each directory as a root (a file argument opens the file and adds its
  git root / parent as a root, as today). `vr-ide` with no argument keeps today's behaviour (cwd, or the
  git root it is in).
- Commands (palette, no VS Code default key exists): "Workspace: Add Folder…" (path prompt with
  completion, like Save As), "Workspace: Remove Folder" (the root under the explorer cursor, or a pick
  list; never deletes files), "Workspace: Save As…" (writes a `.vr-workspace` TOML file listing the roots,
  relative paths when inside the file's folder), "Workspace: Open…" (opens a `.vr-workspace` file;
  `vr-ide my.vr-workspace` too).
- Explorer: one top-level node per root (name = folder name, disambiguated with the parent when two
  roots share a name), each expandable as today; hidden/ignored toggles apply per root (.gitignore of
  that root); file operations stay inside the root they start in (D109 confinement per root); moving
  between roots is refused with a notice (copy is allowed).
- Go to file: indexes every root; results show `root-name/relative/path` when more than one root.
- Project search and replace: run over all roots (one worker, roots in order); results grouped by root,
  then by file; the replace preview and confirm work unchanged across roots.
- File watching: per root as today (non-recursive per expanded folder, D110).
- Terminal (M5): a new terminal starts in the root of the active file, or the first root; the
  "New Terminal" palette entry offers a root pick when there are several.
- Status bar / welcome: the welcome PROJECT block lists the roots (name, branch, file count); the status
  bar shows `root-name/relative/path` when more than one root.
- Recent workspaces: the last 10 opened root sets (or .vr-workspace files) are kept in
  `$XDG_STATE_HOME/vr-ide/recent.toml`; the welcome lists them and the palette "Workspace: Open Recent"
  reopens one (replacing the current roots after the usual unsaved-files prompt).
- Unsaved-file and recovery behaviour unchanged; recovery entries record which root a file belongs to.

## Out of scope
Per-root settings overrides, per-root keymaps, multi-window, remote roots.

## Acceptance criteria
- AC-W1: `vr-ide a b` shows two roots in the explorer; go to file finds files from both; opening one
  shows `a/…` or `b/…` in the status bar.
- AC-W2: Project search finds matches in both roots, grouped by root; project replace previews and
  applies across both; each root's .gitignore is honoured only inside that root.
- AC-W3: Add Folder adds a root live (explorer, index, search, watch); Remove Folder removes it without
  touching files on disk and closes nothing that is open (open tabs from it keep working).
- AC-W4: Save As / Open a `.vr-workspace` round-trips the roots; Open Recent reopens a set from the welcome.
- AC-W5: Explorer move from root a to root b is refused; copy works; trash/rename stay confined to the root.
- AC-W6: A new terminal starts in the active file's root.
- AC-1 still holds with one root (startup < 100 ms, RSS < 60 MB); with 3 roots of 5k files each, startup
  first frame < 100 ms (indexing in the background).

## Edge cases
Nested roots (a root inside another) — refused with a notice; the same folder twice — ignored; a root
deleted on disk — shown as missing, removable; symlinked roots — resolved, de-duplicated; very many roots
(20) — explorer and search stay responsive.
