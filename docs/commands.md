# Slash commands

Open the command window with `F1` (anywhere) or `space` `space` (read mode), then type `/`.
The list shows the matching slash commands with their arguments and a short description.

- `Tab` completes the command name, then the argument: paths for `/save-as`, `/ws add`,
  `/ws save` and `/ws open` (relative to the project folder; `~` is your home folder), the theme
  names, the workspace folders of `/ws remove`, the numbers of `/ws recent` and `/recent`, the
  values of `/indent` and `/eol`, the language ids of `/lang`, and for `/set` the setting names
  (with their value now), then `true` / `false` or the theme names. With several
  matches `Tab` first fills in what they share, then takes the highlighted one (`up`/`down`).
- `Enter` runs the command. On a partial name it takes the highlighted command (a command that
  needs an argument is completed instead).
- A command that does not exist, or a missing or wrong argument, shows an error under the input
  and keeps the text for correction.
- `Esc` closes the window and returns to read (normal) mode.
- Without `/` the window is the command palette and lists the named commands as before.

`/run` and `/ai` only act on your `Enter`. They type the text, then `Enter`, into a terminal of the
integrated panel: the panel's own shell reads it as if you had typed it (vr-ide adds no shell of
its own), so a missing program shows that shell's error. `/run` uses the active terminal when its
shell is waiting for a command; when there is none, its shell has exited or a program started from
it holds the terminal (an AI tool, an editor, a password prompt), it opens a new terminal tab and
runs the command there, with a notice. `/ai` always opens a new terminal tab.

`/ws save` without a name saves to the workspace's own file only when that file is still the
version the workspace was opened from or last saved to; an edit made since (by hand or by another
vr-ide window) is refused, never overwritten. `/q!` quits without asking about unsaved files (a
copy goes to the recovery folder); running terminals still ask first, and "Quit anyway" then still
discards.

`/theme` changes the theme for this session; set `theme` in `settings.toml` to keep it.
`/term right` and `/term bottom` move the terminal panel for this session, as `ctrl+f7`
(Terminal: Toggle Panel Position) does; set `terminal_position` in `settings.toml` to keep it.
On the right the panel takes `terminal_width_percent` of the editor column (drag the separator,
or `ctrl+f8` / `ctrl+f9`, to change it) and falls back to the bottom while the editor column is
under 60 columns wide. A new terminal and closing one: the `+` and the `x` of each tab in the
panel's tab bar (no slash command, no key).

Git (read only): in a folder that is a git repository (or inside one) the gutter marks the lines
of the active file that differ from the git index, unsaved changes included: a bar for added and
changed lines, a low or high bar where lines were removed. The status bar shows the branch (or
`detached <sha>`), `↑n ↓n` against its upstream, and `parent repo: <folder>` when the repository
was found above the workspace folder (e.g. a dotfiles repo in your home folder). Each workspace
folder uses its own repository; a folder without git shows nothing. `/git blame line` turns on a
dim `Author, 3 days ago • subject` after the cursor line once the cursor rests there (`You,
uncommitted` for lines not committed yet); it hides while you type. It is off by default and
changes for this session; set `git_inline_blame = true` in `settings.toml` to keep it on.

Git popup: `/git` opens it on the repository of the active file's workspace folder (or of the
first folder when no file is open); `/git <name>` on the workspace folder with that name (names
only: `/git ../x` says "no such root"). The popup stays on that repository; opening it again keeps
your message draft. `/git push` asks "Push main of ~/repo to origin/main?" (with "and make it the
upstream" for a branch without one) and pushes in the background; `/git pull` pulls in the
background. Both show the popup with git's progress; a second `/git push` or `/git pull` while
one runs offers to cancel it. A pull that stops on conflicts leaves the merge to the terminal and
the popup says so. Keys inside the popup are listed in `docs/keymap.md`.

Git history and blame (popup tabs 2 and 3): `/git log` shows the repository's commits (short id,
date, author, subject), newest first, loading more as you scroll; `/git log file` the commits of
the active file, following renames (as `git log --follow`). `enter` opens the commit view: full
id, author, date (UTC), the whole message, the changed files and the diff against the first parent,
syntax-coloured; `esc` goes back to the list. `/git blame` shows the active file (your unsaved
text included) with the author and age of each block of lines, newest commits brightest; lines not
committed yet show `+ You, uncommitted`. A blame of a long-lived file can take seconds: the tab
says `blaming…` and every key keeps working. `3` from the Changes tab blames the file selected
there; `enter` on a line opens its commit. `/git show <commit>` opens the commit view for an id,
branch, tag or `HEAD~2`; a name git cannot resolve to exactly one commit is refused ("no such
commit", or "the short id matches more than one commit").

File commands act on the active tab. `/rename <name>` renames its file in the same folder with the
explorer's checks (one name, no `/`; an existing name is never replaced) and the tab follows,
unsaved changes included. `/copy-relpath` copies the path relative to the workspace folder that
holds the file. `/recent` lists the last 20 files opened in any vr-ide window (kept in
`~/.local/state/vr-ide/recent-files.toml`, written in the background); `/recent 3` opens the
third, `/recent main.rs` the newest whose path contains `main.rs`.

Text commands change the selected lines (`/sort`, `/unique`, `/trim`, `/indent`) or the selected
text (`/upper`, `/lower`), or the whole file when nothing is selected. Like typing they need edit
mode (`space` `i`); in read mode they show the read-mode hint and change nothing. Each one is a
single undo step. `/unique` keeps the first of equal lines; `/trim` drops trailing spaces and tabs.
`/indent 2|4|tab` re-indents: the old indent step is the most common indent increase between
lines of the whole file (blank lines and block-comment lines such as ` * text` are skipped; a
selection uses the step of the whole file too), each step becomes the new unit and columns left
over stay spaces; the file then indents with that unit. `/eol lf|crlf` converts every line break
and marks the file changed; in a file with no line break yet it sets the break that Enter and
paste insert. `/lang <id>` highlights the file as that language until the tab closes.

`/set <name> <value>` changes one `settings.toml` setting for this session (the file is not
written); the value is checked as in the file (type, range, theme names). Only that setting is
applied: explorer toggles and the terminal height you changed stay as they are.
`/config` and `/keymap` open your `settings.toml` and `keymap.toml`; a missing one is first
made from the built-in defaults with every line commented out, so it changes nothing until you
uncomment a line. Saving either file, in vr-ide or in another editor, reads it again at once:
of `settings.toml` only the settings whose value in the file changed are applied (as `/set`
does, so a `/set` or a toggle of this session on another setting stays); `keymap.toml` replaces
the keys in use. A file with an error (bad TOML, an unknown name, a wrong value) changes
nothing: the old values stay and the error is shown. `/keys` shows the keys of the keymap in use in a read-only tab (keys a terminal without the kitty keyboard
protocol does not send are marked).

<!-- slash-commands:start (generated from the registry; a test checks it) -->
| Command | What it does |
|---|---|
| `/save-as [path]` | Save under another name (no path: the Save As prompt) |
| `/reload` | Reload the file from disk |
| `/rename <name>` | Rename the file on disk (the tab follows) |
| `/copy-path` | Copy the file's full path |
| `/copy-relpath` | Copy the path within its workspace folder |
| `/recent [n\|name]` | Open a recent file (no argument: list them) |
| `/sort` | Sort the selected lines (none: the file) |
| `/unique` | Drop repeated lines (selection or file) |
| `/upper` | Upper-case the selection (or the file) |
| `/lower` | Lower-case the selection (or the file) |
| `/trim` | Drop trailing spaces (selection or file) |
| `/indent 2\|4\|tab` | Re-indent; set the file's indent unit |
| `/eol lf\|crlf` | Convert the file's line breaks |
| `/lang <id>` | Highlight the file as another language |
| `/term right` | Terminal panel on the right, for this session |
| `/term bottom` | Terminal panel at the bottom, for this session |
| `/run <command>` | Type the command and Enter into the terminal (opens one) |
| `/ai <tool>` | Run an AI tool (e.g. claude) in a new terminal tab |
| `/ws add <dir>` | Add a folder to the workspace |
| `/ws remove [root]` | Remove a workspace folder (default: the active file's) |
| `/ws save [file]` | Save the workspace folders to a .vr-workspace file |
| `/ws open <file>` | Open a .vr-workspace file |
| `/ws recent [n]` | Open a recent workspace (no number: list them) |
| `/theme synthwave\|dracula\|dark\|light` | Switch the colour theme for this session |
| `/hidden` | Show or hide hidden files in the explorer |
| `/ignored` | Show or hide ignored files in the explorer |
| `/set <name> <value>` | Change a setting for this session |
| `/config` | Open settings.toml (made when missing) |
| `/keymap` | Open keymap.toml (made when missing) |
| `/keys` | Show the key table (read-only tab) |
| `/git blame line` | Show or hide the cursor line's blame, for this session |
| `/git [root]` | Git popup: changes, stage, discard, commit, push (default: the active file's repo) |
| `/git log` | Git popup, History tab: the repo's commits (enter shows one) |
| `/git log file` | Git popup, History tab: the active file's commits, following renames |
| `/git blame` | Git popup, Blame tab: author and age of each line of the active file |
| `/git show <commit>` | Git popup: one commit's details and diff (an id, branch, tag or HEAD~2) |
| `/git push` | Push the current branch in the background (the target is shown first) |
| `/git pull` | Pull in the background (conflicts are resolved in the terminal) |
| `/help` | List the slash commands |
| `/q` | Quit (asks about unsaved files) |
| `/q!` | Quit, discarding unsaved changes (copy kept in recovery) |
<!-- slash-commands:end -->
