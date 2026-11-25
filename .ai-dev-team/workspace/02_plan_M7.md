# Plan — M7 Git display (GitLens-style)

Spec: `.ai-dev-team/workspace/01_spec_M7.md`.

## Architecture
- New crate `crates/vr-git` (read-only, no UI types): `Repo::discover`, `blame(path) -> Vec<LineBlame>`,
  `blame_text(path, buffer)` (uncommitted lines marked), `line_changes(path, buffer) -> Vec<Change>` (gutter),
  `file_log(path)`, `commit(id) -> CommitInfo` + `commit_file_diff(id, path)`, `head()` (branch/detached),
  `ahead_behind()`. gix first; read-only git CLI fallback where gix is too slow or missing (decided with
  evidence in M7-1). A `GitWorker` per repo: versioned results, cache invalidated on HEAD/index/file change.
- vr-ide: gutter marker column, inline end-of-line blame annotation, status bar segment, file blame view
  (left column), commit details popup, file history list + read-only diff tab.

## Tasks (updated for the Git GUI addition and D255)
0. M7-TM (reviewer, opus) threat model for the write side → `02_threat-model_M7.md` (before M7-2).
1. M7-1 (be, opus) vr-git read API + worker + caches + perf fixtures (50k files, 5k commits) + edge cases;
   status lists (unstaged/staged with rename/untracked), index-vs-worktree and HEAD-vs-index diffs as hunks.
2. M7-2 (be, opus) vr-git write API via the `git` CLI only: stage/unstage file, apply hunk or line patch
   (`git apply --cached [-R]`), revert file/hunk with recovery copy, commit (message via stdin/file, amend,
   sign-off), push (background, progress, verbatim errors); GIT_TERMINAL_PROMPT=0, no shell, `--`; AC-16f tests
   against real `git` in temp repos. After M7-TM.
3. M7-3 (fe, opus) GitLens display: gutter, inline blame, status bar, file blame, commit details, file history;
   commands `:git blame`, `:git show`, `:git log`, `:git blame line` (after W-2 and T20).
4. M7-4 (fe, opus) Git GUI view, `:git gui` (after M7-2, M7-3).
5. Sign-off qc + Phase 5 (correctness + security + ui).
Status: DRAFT, not approved. The user proposes M7 and T20 step by step; nothing is built until the user approves (M7-1/M7-TM agents were started without approval and stopped; branch m7-1-vr-git kept, unmerged).
