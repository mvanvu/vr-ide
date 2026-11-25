# Slash commands

Open the command window with `F1` (anywhere) or `space` `space` (read mode), then type `/`.
The list shows the matching slash commands with their arguments and a short description.

- `Tab` completes the command name, then the argument: paths for `/save-as`, `/ws add`,
  `/ws save` and `/ws open` (relative to the project folder; `~` is your home folder), the theme
  names, the workspace folders of `/ws remove` and the numbers of `/ws recent`. With several
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
`/term right`, `/term bottom` and `/wrap` are listed but not available yet.

<!-- slash-commands:start (generated from the registry; a test checks it) -->
| Command | What it does |
|---|---|
| `/save-as [path]` | Save under another name (no path: the Save As prompt) |
| `/reload` | Reload the file from disk |
| `/term new` | Open a new terminal tab |
| `/term close` | Close the active terminal tab |
| `/term right` | Terminal panel on the right (not available yet) |
| `/term bottom` | Terminal panel at the bottom (not available yet) |
| `/run <command>` | Type the command and Enter into the terminal (opens one) |
| `/ai <tool>` | Run an AI tool (e.g. claude) in a new terminal tab |
| `/ws add <dir>` | Add a folder to the workspace |
| `/ws remove [root]` | Remove a workspace folder (default: the active file's) |
| `/ws save [file]` | Save the workspace folders to a .vr-workspace file |
| `/ws open <file>` | Open a .vr-workspace file |
| `/ws recent [n]` | Open a recent workspace (no number: list them) |
| `/theme synthwave\|dracula\|dark\|light` | Switch the colour theme for this session |
| `/wrap` | Toggle soft line wrap (not available yet) |
| `/hidden` | Show or hide hidden files in the explorer |
| `/ignored` | Show or hide ignored files in the explorer |
| `/help` | List the slash commands |
| `/q` | Quit (asks about unsaved files) |
| `/q!` | Quit, discarding unsaved changes (copy kept in recovery) |
<!-- slash-commands:end -->
