# M7 Git — leader proposal (asked by the user 2026-11-25 "propose it yourself"; NOT approved, nothing built)

## How to open (command window F1 / space space; no new default keys)
| Command | Opens |
|---|---|
| `/git` | Git popup, Changes tab (like `git gui`) |
| `/git log` | History tab: repo commits |
| `/git log file` | History of the active file |
| `/git blame` | Blame tab for the active file |
| `/git show <sha>` | One commit's details and diff |
| `/git push`, `/git pull` | Run in the background, progress shown, git's own errors verbatim |

## Git popup (about 90 % of the screen, Esc closes)
```
┌ Git ─ vr-ide · master ↑2 ↓0 ──────── [Changes] History  Blame ─┐
│ Unstaged (3)                 │ src/app.rs                       │
│  M src/app.rs                │ @@ -10,6 +10,8 @@                │
│  ? notes.md                  │ - old line                       │
│ Staged (1)                   │ + new line                       │
│  M README.md                 │                                  │
├──────────────────────────────┴──────────────────────────────────┤
│ Message: Fix the find box_                [ ] Amend  [ ] Sign off│
│ [Commit]  [Push]  [Rescan]                                       │
└──────────────────────────────────────────────────────────────────┘
```
- Changes: stage/unstage a file, a hunk or the selected lines; discard asks first and keeps a recovery copy;
  commit message box, Amend, Sign off, Commit, Push, Rescan.
- History: commits (date, author, subject); Enter shows the commit's diff.
- Blame: author and age per line, coloured by age; Enter opens that commit.
- Mouse: click lists, buttons and tabs.

## Keys inside the popup only (proposal — the user decides)
| Key | Action |
|---|---|
| `1` / `2` / `3` | Changes / History / Blame tab |
| `tab` | Move between file lists, diff and message box |
| `space` | Stage / unstage the selected file |
| `enter` | Stage / unstage the selected hunk (or selected lines) |
| `d` | Discard (asks first) |
| `c` | Go to the message box |
| `ctrl+s` | Commit (in the message box) |
| `p` | Push |
| `r` | Rescan |
| `esc` | Close the popup |

## In the editor (light GitLens-style, optional — the user decides)
- Gutter marks for added / modified / deleted lines.
- Status bar: branch and ahead/behind.
- `/git blame line` toggles a dim "author, 3 days ago • subject" at the end of the cursor line.

## Multi-root
The popup works on the repo of the active file; the header names it; `/git <root-name>` switches repo.

## Safety
Every write runs the `git` CLI (no shell, `--` before paths, GIT_TERMINAL_PROMPT=0, credentials only through
git's own helpers / ssh-agent); a threat model is written and reviewed before any write code.

## Order if approved
1. M7-1 read library (status, diff hunks, log, blame, branch / ahead-behind) — worktree branch m7-1-vr-git exists, unmerged.
2. Threat model + M7-2 write library (stage / unstage / hunk and line patches / discard / commit / push / pull).
3. M7-3 Git popup, Changes tab.
4. M7-4 History and Blame tabs + editor gutter / status bar / line blame.
5. Sign-off QC + final review.

## Questions for the user
1. Approve the command list and the popup layout?
2. Keys inside the popup as above, or other keys?
3. Include `/git pull`? (pull can create merge conflicts; conflict resolution would stay in the terminal)
4. The editor part (gutter, status bar, line blame): yes / no / later?
