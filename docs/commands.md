# Slash commands

Open the command window with `F1` (anywhere) or `space` `space` (read mode), then type `/`.
The list shows the matching slash commands with their arguments and a short description.

- `Tab` completes the command name, then the argument: paths for `/save-as`, `/ws add`
  (folders), `/ws save` and `/ws open` (folders and `.vr-workspace` files), relative to the
  project folder (`~` is your home folder); the file arguments of `/run` (after the program);
  for `/ssh` the files of `-i`, `-F`, `-E`, `-S` and `-o IdentityFile=` (relative to the
  workspace folder of the active file, as ssh resolves them) and, in the destination position,
  the recent targets and the hosts of `~/.ssh/config`; the tool names of `/ai` (setting
  `ai_tools`); the theme names, the workspace folders of
  `/ws remove`, the numbers of `/ws recent` and `/recent`, the values of `/indent` and `/eol`,
  the language ids of `/lang`, and for `/set` the setting names (with their value now), then
  `true` / `false` or the theme names. Hidden files are offered only when what you typed starts
  with `.`; a folder is read up to 1000 entries and at most 100 are offered.
- One match is completed (a folder ends with `/`; a finished `/ssh`, `/run` or `/ai` word gets a
  space). Several: `Tab` fills in what they share and the input shows `tab: list (N)`; the next
  `Tab` (or the first, when nothing is shared) turns the list into the candidates with `i/N` beside
  the input. There `Tab` / `shift+tab` (or `up` / `down`) step through them, `Enter` takes the
  candidate without running it, `Esc` goes back to the text from before the list (a second `Esc`
  closes the window) and typing closes the list and edits. A row highlighted with `up` / `down`
  before `Tab` is taken as it is (not in `/ssh`, where `Tab` completes the word being typed).
- `Enter` runs the command. On a partial name it takes the highlighted command (a command that
  needs an argument is completed instead).
- A command that does not exist, or a missing or wrong argument, shows an error under the input
  and keeps the text for correction.
- `Esc` closes the window and returns to read (normal) mode.
- Without `/` the window is the command palette and lists the named commands as before.

`/run` only acts on your `Enter`. It types the text, then `Enter`, into a terminal of the
integrated panel: the panel's own shell reads it as if you had typed it (vr-ide adds no shell of
its own), so a missing program shows that shell's error. `/run` uses the active terminal when its
shell is waiting for a command; when there is none, its shell has exited or a program started from
it holds the terminal (an AI tool, an editor, a password prompt), it opens a new terminal tab and
runs the command there, with a notice. `/ai <tool>` always starts a new session in the AI panel,
never in a terminal; `/ai` alone opens the tool picker there, as `ctrl+f12` does.

`/ws open <folder>` switches the window to that folder alone, `/ws open <file>.vr-workspace` to the
folders the file lists; any other file is refused. Unsaved files are asked about first (Save all /
Discard, which keeps a copy in the recovery folder / Cancel), then every tab closes; terminals and
AI sessions keep running in their folders. Opening the folder that is already the workspace
changes nothing. The new workspace goes to the front of `/ws recent`.

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
panel's tab bar, or `ctrl+n` / `ctrl+w` in a focused terminal (no slash command).

`/ssh` (also: `ssh` and a space, or SSH: Connect to Host in the command list) lists the open SSH
sessions, the recently used lines and the `Host` names of `~/.ssh/config`; type what you would
type after `ssh ` in a terminal (`-i credentials/key user@host`, `-p 2222 user@host`,
`-J jump host`; quotes and `\` work, no shell runs it) and press Enter on its row. Relative paths
resolve in the workspace folder of the active file; the window shows `runs: ssh …` with them
resolved. The session starts in a terminal tab and takes the whole window once the login
succeeded; see "SSH sessions" in the README.

Git (read only): in a folder that is a git repository (or inside one) the gutter marks the lines
of the active file that differ from the git index, unsaved changes included: a bar for added and
changed lines, a low or high bar where lines were removed. The status bar shows the branch (or
`detached <sha>`), `↑n ↓n` against its upstream, and `parent repo: <folder>` when the repository
was found above the workspace folder (e.g. a dotfiles repo in your home folder). Each workspace
folder uses its own repository; a folder without git shows nothing. Set `git_inline_blame = true`
in `settings.toml` for a dim `Author, 3 days ago • subject` after the cursor line once the cursor
rests there (`You, uncommitted` for lines not committed yet); it hides while you type. It is off
by default.

Blame annotations (`shift+f2`, palette "Toggle Blame Annotations"): a column left of the line
numbers shows the author and the commit date (`YYYY-MM-DD`, UTC) on the first line of each run of
lines from one commit and a thin `┊` on the rest; lines not committed yet show `Not committed yet`.
It is on or off for the whole session and every editor tab, in read and edit mode, next to the
gutter marks and the inline blame. While you edit, each annotation stays on its line and new or
changed lines show as not committed; saving reads the blame from git again. The column needs room
beside the text: when the editor leaves fewer than 72 cells next to the line numbers (about 76
columns of editor with no side panel; open panels and a wider line-number gutter take more) the
author is shortened, below 50 cells the column hides.
A file outside a repository, an untracked file or one over the size cap shows a notice instead. Stage, commit, push, pull and
history are done with git in a terminal tab.

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
| `/ai [<tool> [@account]]` | Start an AI tool of ai_tools (e.g. claude) in the AI panel (none: pick one); /ai rename <name> |
| `/ssh [ssh args]` | Open an SSH session, args as after ssh (none: list hosts) |
| `/ws add <folder>` | Add a folder to the workspace |
| `/ws remove [root]` | Remove a workspace folder (default: the active file's) |
| `/ws save [file]` | Save the workspace folders to a .vr-workspace file |
| `/ws open <folder\|file>` | Open a folder or a .vr-workspace file as the workspace |
| `/ws recent [n]` | Open a recent workspace (no number: list them) |
| `/theme synthwave\|dracula\|dark\|light` | Switch the colour theme for this session |
| `/hidden` | Show or hide hidden files in the explorer |
| `/ignored` | Show or hide ignored files in the explorer |
| `/set <name> <value>` | Change a setting for this session |
| `/config` | Open settings.toml (made when missing) |
| `/keymap` | Open keymap.toml (made when missing) |
| `/keys` | Show the key table (read-only tab) |
| `/help` | List the slash commands |
| `/q` | Quit (asks about unsaved files) |
| `/q!` | Quit, discarding unsaved changes (copy kept in recovery) |
<!-- slash-commands:end -->
