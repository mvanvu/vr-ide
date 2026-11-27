# Spec — M7 Git, display only (GitLens-style)

flags: has_ui=true (terminal UI), security_sensitive=false (read-only; no git writes), platforms=linux
source: user 2026-11-25 "keep it simple, just show it like GitLens" — replaces the earlier full-git draft
(panel, staging, commit, push are out). Writes stay in the terminal panel (the user runs git there).

## Goal
Show git context while reading and editing code, like the VS Code GitLens extension, without any git writes.

## Scope
- Current-line blame (inline): after the end of the cursor line, a dim annotation
  `  Author, 3 days ago • commit summary` (uncommitted lines: `  You, uncommitted`). Shown after a short
  pause of the cursor (~300 ms), hidden while typing; setting to turn it off.
- Gutter change markers: added / modified / deleted lines against the index (HEAD when not staged),
  computed off the UI thread, updated after edits (unsaved text vs index) and on file-system changes.
- Status bar: branch (or detached short SHA), ahead/behind vs upstream, and the blame of the cursor line in
  short form when there is room.
- Whole-file blame view (toggle): a left column with author + relative date per block of lines, coloured by
  age (newest brightest), like GitLens "file blame".
- Commit details: on the cursor line, a command shows a popup with full SHA, author, date, full message and
  the file's diff for that commit (read-only, syntax-coloured).
- File history: a list of commits that touched the current file (date, author, summary); Enter opens that
  commit's diff for the file in a read-only tab.
- Line history (optional, if cheap): commits that touched the selected lines (`git log -L`-style via gix or CLI).
- Multi-root: each root's repo; a folder without git shows nothing (no errors).

## Out of scope
Staging, discard, commit, branch switch, pull/push/fetch, git panel, conflict resolution — use the terminal panel.

## Implementation constraints
- Read with `gix` (blame, log, diff, status, branches, ahead/behind); if gix lacks a needed feature with
  acceptable speed (e.g. blame), fall back to a read-only `git` CLI call (`git blame --porcelain`,
  `git log --follow`) with GIT_TERMINAL_PROMPT=0, no shell, bounded time. Never a write command.
- Everything off the UI thread with versioned results; cached per file and invalidated on HEAD/index/file change.

## Keys (USER-5: ctrl or F-keys, one per command) — to be approved by the user
No default keys (D255): command-line names `:git gui`, `:git blame` (file blame toggle), `:git show` (commit
details of the cursor line), `:git log` (file history), `:git blame line` (current-line blame on/off), also in
the F1 palette. Inside the Git GUI view, local keys (Space stage/unstage file, Enter hunk, ctrl+l lines,
Delete revert, ctrl+s commit) are view-local, not global. Scope approved (D255).

## Acceptance criteria
- AC-16 (display): on a repo with history, the cursor line shows author/relative date/summary within 500 ms
  of resting; uncommitted lines show "You, uncommitted".
- AC-16a: gutter markers match `git diff` (index vs worktree, plus unsaved buffer edits) for added/modified/deleted lines.
- AC-16b: status bar shows branch and ahead/behind matching `git status -sb`; detached HEAD shows the short SHA.
- AC-16c: file blame view matches `git blame --porcelain` authors/commits per line; file history matches
  `git log --follow -- file`; commit details match `git show`.
- AC-16d: a folder without git, a bare/empty repo and a file not tracked show nothing and no errors; multi-root works per root.
- AC-16e: 50k-file repo and a 5 000-commit file history: UI never blocks; blame of a 5 000-line file < 1 s off-thread.
- AC-1 still holds: no git work at startup on the UI thread.

## Addition (user 2026-11-25 "add a simulation like the `git gui` command")
A "Git GUI" view modelled on `git gui` (Tk), opened as a full editor-area view (not a side column):
- Left top: **Unstaged Changes** list; left bottom: **Staged Changes (Will Commit)** list — status icon + path.
- Right: diff of the selected file (unstaged → worktree vs index; staged → index vs HEAD), syntax-coloured.
- Actions as in git gui: stage/unstage the selected file; stage/unstage the hunk or the selected **lines**
  under the cursor in the diff; revert (discard) a file/hunk with confirmation (Cancel default, recovery copy);
  "Rescan".
- Bottom: commit message box (multi-line), **Amend Last Commit** toggle (loads the last message), **Sign Off**,
  **Commit**, **Push** (to the upstream; background with progress; errors shown verbatim).
- Branch shown in the header; current branch only (switching via the terminal).
- Writes go through the `git` CLI (add / restore --staged / `git apply --cached [-R]` for hunks and lines /
  commit / push) with GIT_TERMINAL_PROMPT=0, no shell, argument-safe paths (`--` separators); credentials via
  git's own helpers/ssh-agent only. security_sensitive becomes true for this part (threat model before build).
- AC-16f: every stage/unstage (file, hunk, line), revert, commit, amend and push leaves the repo exactly as the
  same `git` commands would (verified with `git status --porcelain=v2`, `git diff --cached`, `git log -1`).
