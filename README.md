# vr-ide

A lightweight, non-modal terminal IDE written in Rust: no modes, one key per command, and a
small memory footprint. It edits files in a tabbed editor next to a file explorer, with find,
replace, project search and a command palette.

Contents: [Requirements](#requirements) · [Install](#install) · [Quick start](#quick-start) ·
[Using vr-ide](#using-vr-ide) · [Configuration](#configuration) ·
[Troubleshooting](#troubleshooting) · [Recovery](#recovery) · [Safety model](#safety-model) ·
[Terminal notes](#terminal-notes) · [Scope and roadmap](#scope-and-roadmap) ·
[Benchmark](#benchmark)

## Requirements

- **Linux.** It is the supported platform. The code compiles elsewhere (macOS), but that is not a
  target of v1.
- **Rust, stable channel** (the project uses edition 2024, so a recent stable release). Install it
  with [rustup](https://rustup.rs):

  ```
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

  rustup puts `cargo` in `~/.cargo/bin`. The installer normally adds that folder to your `PATH`
  through your shell profile; open a new terminal afterwards. If you passed `--no-modify-path`
  (`sh -s -- --no-modify-path`) or `cargo` is "not found", add it yourself:

  ```
  export PATH="$HOME/.cargo/bin:$PATH"     # put this line in ~/.bashrc, ~/.zshrc, ...
  ```

  Check with `cargo --version`.
- **git** to fetch the source.
- **WebKitGTK 4.1 and GTK 3** (development packages to build, the libraries to run), for the
  [login window](#login-window) of the AI panel. On Debian / Ubuntu:

  ```
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev
  ```

  Without them, build without the login window: add `--no-default-features` to the `cargo`
  commands below. claude then opens its sign-in page in your own browser, as it does outside
  vr-ide.
- **A terminal** with keyboard-enhancement support (the kitty keyboard protocol: kitty, WezTerm,
  foot, Ghostty, recent Alacritty and others) is not required: every default key works without it.
  See [Terminal notes](#terminal-notes).

## Install

From source:

```
git clone https://github.com/mvanvu/vr-ide.git
cd vr-ide
cargo install --path crates/vr-ide
```

The binary is installed to `~/.cargo/bin/vr-ide` (or `$CARGO_HOME/bin`), so it is on your `PATH` when
`cargo` is. To install somewhere else, add `--root <dir>`: the binary lands in `<dir>/bin/vr-ide`
and you add that `bin` folder to your `PATH`.

Build without installing:

```
cargo build --release
./target/release/vr-ide
```

Desktop launcher (GNOME and other freedesktop desktops): `vr-ide --install-desktop` adds an app-menu entry
and icon under `~/.local/share` (honours `$XDG_DATA_HOME`) that opens vr-ide in a terminal window;
`vr-ide --uninstall-desktop` removes exactly those two files. Run it again after moving the binary.

Update: pull the new source and install again; `cargo install` replaces the old binary.

```
cd vr-ide
git pull
cargo install --path crates/vr-ide
```

Uninstall:

```
cargo uninstall vr-ide        # add --root <dir> if you installed with --root
```

This removes only the binary. Your settings and keymap (`~/.config/vr-ide`) and the recovery files
(`~/.local/state/vr-ide`) stay; delete them yourself if you want them gone.

## Quick start

```
vr-ide              # open the current directory as the project root
vr-ide <dir>        # open <dir> as the project root
vr-ide <file>       # open <file>; the project root is its enclosing git root
```

- The explorer starts hidden: `ctrl+e` shows it with the keys in the tree (arrows move, `enter`
  opens) and hides it again.
- The explorer colours names by kind from the theme: folders in one colour, files by group (source
  code by language family, config/data, docs, images/media, archives/binaries, lock/generated;
  other dotfiles dimmed and italic).
- A file opens in **read mode** (status bar `READ`): you can move, select, copy, find and save, but
  typing does nothing. Press `space` then `i` to edit (`EDIT`) and `esc` to go back to reading;
  `space` `space` opens the command palette and `tab` moves the focus to the next part. A new file
  from `ctrl+n` opens ready to type.
- With no file open, the welcome dashboard shows the project, the keys to start with and, on Linux,
  a live resource monitor (CPU with history and per core, RAM, swap, load, uptime, vr-ide's own CPU
  and memory). It is sampled once a second only while it is on screen.
- The **project root** is the folder the explorer shows and the one project search, "Go to File" and
  relative paths in prompts refer to (with several folders, see
  [Workspace](#workspace-several-folders)).
- With no argument it is the current directory; with a directory it is that directory (the path
  is resolved, so `a/../a/b` works).
- With a file it is the nearest parent folder that contains `.git`, walking up from the file's
  folder; if there is none, the file's own folder.
- vr-ide has one option, `--session` (alone); anything else, `vr-ide --help` or `--version`
  included, is read as a path and fails. A missing path prints a message and exits with status 2.
- `vr-ide --session` (what the desktop launcher runs) reopens the last session: the workspace
  folders, the open file tabs (cursor, scroll, read/edit mode), the active tab, the explorer's
  expanded folders and the terminal panel (shown, position, size); each terminal starts as a fresh
  shell in its folder and nothing is re-run. Files are read as they are on disk now; a file that is
  gone is skipped with one notice. With no saved session it opens your home folder. The session is
  saved on quit and about a second after each change to `~/.local/state/vr-ide/session.toml`
  (mode 0600; unsaved text is never stored). Only one `--session` window saves it; a second one says
  so. An unreadable file gives one notice, opens the home folder and is kept as
  `session.toml.bad`. Plain `vr-ide` and `vr-ide <folder|file|.vr-workspace>` never read or write
  the session.
- It needs an interactive terminal; it cannot run with its input or output redirected.

Press `F1` to open the command palette, which lists every command with its
key. A row whose description is cut shows it in full (up to 3 lines) under the list.
Press `ctrl+q` to quit. If any file has unsaved changes, vr-ide asks first (see
[Quit guard](#quit-guard)).

### Workspace (several folders)

- `vr-ide dir1 dir2 …` opens each folder as a root of one workspace; a file argument opens the file
  and adds its project as a root unless a folder argument already holds it.
- `vr-ide team.vr-workspace` opens the roots listed in that file; a workspace file is given alone
  (with other arguments vr-ide prints the usage and exits with status 2).
- The explorer then shows one node per folder; a folder that is gone is marked `(missing)` and can
  be removed. Each file operation stays inside its own folder: moving (cut and paste, Move) between
  workspace folders is refused, also through a folder link that leads into another one; copy and
  paste works across them.
- `/ws add`, `/ws remove`, `/ws save`, `/ws open` and `/ws recent` (also in the palette as
  `Workspace: …`) change the folders, see [docs/commands.md](docs/commands.md).
- `/ws open <folder>` switches the window to another folder, `/ws open <file>.vr-workspace` to
  the folders of a workspace file (so do `/ws recent` and a recent workspace on the welcome
  screen). Open terminals and AI sessions run in the old folders: "Switching workspace closes 2
  terminals and 1 AI session." asks first (Continue / Cancel, Cancel is the default), then unsaved
  files are asked about; Cancel at either step changes nothing. The window title follows.

## Using vr-ide

vr-ide has its own key set: one key per command, each a single key: `ctrl` with a letter, or an F
key alone, with `ctrl` or with `shift` (no `alt`, no `ctrl+shift`, no key sequences), and every key
works in any terminal. Two exceptions: read mode's built-in `space` keys (`space i`, `space space`)
and `ctrl+h` (the terminal sends Backspace as a different byte). The complete list, with command
names, is in
[docs/keymap.md](docs/keymap.md); every key can be overridden ([Configuration](#configuration)).
Commands without a default key can be run from the palette (`F1`).
Typing `/` in the palette runs slash commands (`/run`, `/ai`, `/ws add`, `/theme`, `/help`, ...);
the list is in [docs/commands.md](docs/commands.md).
`tab` completes there: command names, paths (`/ws add`, `/ws open`, `/run` file arguments, the
files of `/ssh -i`), `/ssh` hosts and `/ai` tools; a second `tab` lists the candidates, `tab` /
`shift+tab` step through them, `enter` takes one and `esc` closes only the list.

### Files and tabs

| Task | Keys |
|---|---|
| Open a file by path (relative to the project root) | `ctrl+o` |
| Show a file in the explorer (shows, expands, selects and focuses it) | double left click on its tab (two clicks within 400 ms; tabs without a file do nothing extra), or `ctrl+k` |
| Go to File (fuzzy search by name) | `ctrl+p` |
| New file (created on disk at once; there are no untitled buffers) | `ctrl+n` |
| Save | `ctrl+s` |
| Save As (a new file name, relative to the project root) | palette: Save As |
| Reload the file from disk | palette: Reload File from Disk |
| Close the tab | `ctrl+w`, or left click the `x` at the end of its label (any tab, without switching to it) |
| Next / previous tab | `f9` / `shift+f9` |
| Command palette | `F1` |

Closing a tab with unsaved changes asks: Save, Discard or Cancel. Discard keeps a copy of your
changes in the [recovery folder](#recovery).

### Editing

Files open in read mode, so a stray key never changes them. `space` then `i` switches the tab to
edit mode and `esc` back to read mode (it also clears the selection). In read mode `space` `space`
opens the command palette, `space` followed by any other key only shows a short hint, and `tab`
moves the focus (editor, explorer, terminal; the find box is reached with `ctrl+f` and keeps
`tab` for its fields); in edit mode `tab` / `shift+tab` indent
and outdent. The tab returns to read mode whenever
the editor gets the keys on it again: switching tabs, coming back from the explorer, the find box
or a dialog, or closing the palette without running a command. In read mode, typing and the
editing keys below show the hint "Read mode: space i to edit"; moving, selecting, copying, find,
Go to Line and save work as usual. The status bar shows `READ` or `EDIT`, and where the terminal
supports it the cursor is a block in read mode and a bar in edit mode. Project replace and reloads
from disk still update open files in either mode.

| Task | Keys |
|---|---|
| Edit mode / read mode | `space` `i` (in read mode) / `esc` |
| Command palette from read mode | `space` `space` (or `F1` anywhere) |
| Focus the next part (read mode) | `tab` |
| Undo / redo | `ctrl+z` / `ctrl+y` |
| Copy / cut / paste | `ctrl+c` / `ctrl+x` / `ctrl+v` |
| Select all | `ctrl+a` |
| Extend the selection | hold `shift` with the arrows, `home`, `end`, `pageup`, `pagedown` |
| Move by word / to start or end of the file | `ctrl+left` `ctrl+right` / `ctrl+home` `ctrl+end` |
| Delete line / duplicate line | `ctrl+d` / `ctrl+l` |
| Move line up / down | `ctrl+up` / `ctrl+down` |
| Indent / outdent (edit mode) | `tab` / `shift+tab` |
| Toggle line comment | `ctrl+/` |
| Go to Line | `ctrl+g` |

Go to Line asks for `line` or `line:column` (1-based, for example `42` or `42:7`).

Terminals send `ctrl+[` as `Esc`, so outdent is `shift+tab`.

### Find and replace (file and project)

`ctrl+f` opens one find box under the tab bar with every result listed under it, like
PhpStorm. Its scope is shown on its left: **File** (the default) or **Project**; `ctrl+f` again
in the box switches between them (with no file open it opens on the project). The editor stays
visible under the box and highlights the matches while the box is open.

| Task | Keys |
|---|---|
| Find in the file / switch the scope to the project and back | `ctrl+f` / `ctrl+f` in the box |
| Move in the result list (the editor shows the match) | `up` / `down` / `pageup` / `pagedown` |
| Go to the selected match (read mode; the box stays open) | `enter` |
| Previous match, staying in the box (kitty keyboard protocol terminals) | `shift+enter` |
| Next / previous match or project result | `f4` / `shift+f4` |
| Replace in the box's scope (File: switches the tab to edit mode; find works in read mode) | `ctrl+r` |
| Replace in the file: asks Replace: One / All / Cancel (the box shows "enter replace") | `enter` in the replace field |
| Switch between the find and replace fields | `tab` / `shift+tab` |
| Close the box (back to read mode) | `esc` |

The query says how to search; there are no option buttons:

| Query | Searches for |
|---|---|
| `foo` | the text `foo`, case-sensitive |
| `foo/i` | the text `foo`, ignoring case |
| `/fo+/` | the regular expression `fo+` |
| `/fo+/i` | the regular expression `fo+`, ignoring case |
| `a\/b` | the text `a/b` (`\/` is a literal slash) |

A bad regular expression is shown in the box and nothing is searched.

In File scope the list shows every match as `line  text` (at most 20 000) and follows your
typing. In Project scope press `enter` to search; results stream in as the files are scanned,
grouped by file (and by root when the workspace has several folders), with the count on the box's
first row. `down` moves into the results; `enter` on a result opens the file at the match. Files
matched by `.gitignore` are not searched; hidden files are.

Replacing across files is always previewed first:

1. Press `ctrl+r` in the box on the project (or with no file open, or run Replace in Project from
   the palette), fill in the search and the replace field.
2. Press `enter` in the replace field: the box lists a preview per file. Nothing is written yet.
3. Move through the preview; `space` checks or unchecks a file (unchecked files are left alone).
4. Press `enter` to confirm. A dialog asks Replace or Cancel; Cancel is the default button.
5. `esc` closes the preview without changing anything. While a preview is being prepared or a
   replace is running, `esc` cancels it; the files it has not reached stay as they were and the
   result lists what was changed.

A file that is open in a tab is changed in its buffer as one undoable edit and is not saved for
you; other files are written to disk. A file that changed on disk after the preview is refused,
not overwritten.

### Explorer

| Task | Keys |
|---|---|
| Show the explorer and focus it / hide it (the editor gets the keys) | `ctrl+e` |
| Focus the explorer | `ctrl+b` |
| Reveal the active file in the tree | `ctrl+k` |
| Cycle focus between the parts (from the explorer or the editor in read mode) | `tab` |
| Back to the editor | `esc` |
| Move, expand, collapse | `up` `down` `pageup` `pagedown` `home` `end`; `right` expands, `left` collapses or goes to the parent |
| Open a file / toggle a folder | `enter` |
| New file or folder (asks File / Folder, File first, then the name) | `ctrl+n` |
| Rename | `f2` |
| Delete (asks first, moves to the system trash) | `delete` |
| Copy / cut / paste | `ctrl+c` / `ctrl+x` / `ctrl+v` |
| Show / hide hidden files, ignored files | `ctrl+h` / `ctrl+f6` |

`Explorer: Reveal Active File` expands the tree down to the file in the active tab.
`Explorer: Toggle` (show or hide, focus stays where it is) and `Explorer: Move` (asks for the
destination folder, relative to the project root; with several workspace folders it is typed as
shown in the explorer, `folder-name/path`, and must be in the same folder) are palette only.

By default dotfiles are shown and paths matched by `.gitignore` are hidden. Copy, move and rename
never overwrite an existing item. Mouse clicks and the wheel work in the explorer.

A right click on a row selects it and opens a small menu at the pointer with the same actions as
the keys above: New… (File / Folder), Rename…, Copy, Cut, Paste and Delete, each with its key on
the right (taken from your keymap). Actions that cannot apply are dimmed: Paste while nothing is
copied or cut; Rename, Copy, Cut and Delete on the project folder row. A right click below the last
row targets the project folder (New, Paste); one on a row of a multi-selection keeps the selection.
With several workspace folders, the menu of a folder's root row also has **Remove Folder from
Workspace** (not on the last one); it takes the folder out of the workspace and leaves its files and
open tabs as they are.
On a folder or a workspace folder row (not a file) the menu also has **Search in Folder** and **Replace in Folder** (D328): the find box opens in Project scope limited to that folder, which is shown as the scope (`Project: src/ui`), with the cursor in the search field. Results, the replace preview and the replace itself only touch files under that folder (in a multi-folder workspace, only that folder of its root). `ctrl+f` switches to File scope and back to the whole project, and closing the box ends the folder scope.
`up`/`down` (and `home`/`end`) move, `enter` or a left click runs an item; `esc` or a click outside closes the menu.
Inside a terminal whose program asked for the mouse, the right click still goes to the program.

### External changes

If a file you have open changes on disk:

- With no unsaved changes in the tab, it reloads by itself.
- With unsaved changes, the tab is marked and a prompt asks **Reload** (take the disk version; your
  text is first copied to the [recovery folder](#recovery)) or **Keep mine** (keep your buffer;
  saving then asks again before replacing the file).
- If you save a file that changed on disk since you opened it, the save stops and asks **Keep
  mine** (replace the disk version with yours), **Reload** (take the disk version; your text goes
  to the recovery folder first) or **Cancel** (the default). vr-ide never silently overwrites
  someone else's version.

`Reload File from Disk` and `Save (overwrite)` are available from the palette.

### Quit guard

`ctrl+q` quits at once when everything is saved. With unsaved files it lists them and offers Save
all, Discard or Cancel; Discard keeps a copy in the [recovery folder](#recovery). If a project
replace is still running, `ctrl+q` asks Quit anyway or Cancel (Cancel is the default). While
programs run in the [terminal panel](#terminal) it asks "N terminals still running" with Quit anyway
or Cancel (Cancel is the default); quitting hangs up every terminal's process group (SIGHUP, then
SIGKILL for a program that ignores it).

### Terminal

``ctrl+` `` opens a terminal panel under the editor, running your `$SHELL` (`/bin/sh` if it is
unset or does not start) in the project root with `TERM=xterm-256color`. Press it again, wherever
the keys are, to hide the panel (the shells keep running); click the editor to leave the terminal
with the panel shown. `shift+f12` from the editor hides the panel too. No terminal starts until you
open one. Switching to another workspace (`/ws open`, `/ws recent`) closes every terminal, after
asking (see [Workspace](#workspace-several-folders)).

- A focused terminal receives every key, `ctrl+s` and `ctrl+p` included, so `vim`, `htop`
  or `lazygit` work as in any terminal; vr-ide keeps only ``ctrl+` ``, `F1` (the command window),
  `ctrl+f7` (panel position), `ctrl+f8` and `ctrl+f9` (panel size), `ctrl+n` (a new terminal, like
  the `+` button) and `ctrl+w` (close this terminal, like its `x`; it asks first while a program
  other than the shell runs); the other panel keys work from
  the editor. The status bar shows `TERM` while the terminal has the keys (read and edit mode do
  not apply there).
- The `+` after the tabs opens another terminal (in the active file's workspace folder); the `x`
  at the end of a tab closes that terminal (the last one hides the panel); while a program other than
  the shell runs in it, the `x` first asks "Close terminal?" (Close / Cancel, Cancel is the default). `f8` / `shift+f8` move
  to the next / previous one and `ctrl+f8` / `ctrl+f9` grow / shrink the panel (from the editor or a focused terminal; `f8` goes to the program there).
  The tab bar shows each terminal's title (set
  by the program, or the shell's name) and `[N]` once it exited. Long titles such as bash's
  `user@host: ~/dir` are cut from the left so the directory stays; when the tabs do not fit, the
  active one stays in view and `+N` counts the others. Click a tab to switch; drag a tab along
  the bar to move it (it stays the active one, its number and `f8` / `shift+f8` follow the new
  order; past the last tab shown it moves on one place per cell, past those behind `+N`,
  and left of the first tab shown, at the bar's left edge, it moves toward the front the same
  way); drag the rest of the tab bar to resize the panel at the bottom
  (`terminal_height_percent` in settings, 40 % by default, at least 5 rows).
- `ctrl+f7` (from the editor or a focused terminal; or `/term right`, `/term bottom`) moves the
  panel to the right of the editor for the session: the editor's full height,
  `terminal_width_percent` of the editor column (40 %, at least 20 columns); drag the separator
  or use `ctrl+f8` / `ctrl+f9` to change the width. Under 60 editor columns it shows at the
  bottom. `terminal_position = "right"` in settings keeps it.
- When the program exits the panel says "[process exited N] — Enter to restart, ctrl+w to close"
  (shorter on narrow screens: "exited N · Enter · ctrl+w").
- Notices above the status bar: `Esc` goes to the program while a terminal has the keys, so click
  a notice to dismiss it there. The row shows one notice at a time: a short-lived one (a reply to
  a command) comes before the warnings that stay until dismissed, and each is shown for its
  whole 5 seconds before it goes.
- `shift+pageup` / `shift+pagedown` and the mouse wheel scroll back (10 000 lines, setting
  `scrollback`); a key sent to the program or a paste returns to the bottom, while new output
  leaves a scrolled view where it is (the `↑` indicator shows you are scrolled back). Programs that
  use the mouse get it; hold `shift` to select text anyway.
- Dragging the mouse selects and copies to the clipboard; `ctrl+shift+v` pastes (bracketed when the
  program asked for it), as does your terminal's own paste.
- Colours: `synthwave` and `dracula` give programs their own 16 ANSI colours and background;
  `dark` and `light` leave your terminal's colours for the editor, but the panel itself has a fixed
  background (dark `#3a3a3a`, light `#bcbcbc`) so it reads as a separate area; it may blend in on a
  terminal whose own background is that exact grey.
- With the kitty keyboard protocol `ctrl+space` is a separate key that reaches the program.
- Without it your terminal sends ``ctrl+` `` as NUL, the same byte as `ctrl+space` and `ctrl+@`.
  vr-ide takes NUL as ``ctrl+` `` everywhere, so it hides the panel from inside a terminal too,
  and programs never get `ctrl+space`. If a program needs it (Emacs's mark, a shell binding), set
  `terminal_pass_nul = true`: NUL then goes to the program, a click on the editor leaves the terminal, ``ctrl+` ``
  from the editor shows or hides the panel, and `shift+f12` from the editor hides it. The tab bar's
  hint shows the way out that works in your setup.

### AI panel

`f12` shows a column right of the editor, the window's full height, for terminal coding agents
(claude, codex, gemini, aider, cursor-agent: the `ai_tools` setting; only the ones found on `PATH`
are offered). Each tab is one agent session, and several sessions of one tool can run at once
(`1 claude`, `2 claude·2`). A session starts in the active file's workspace folder. Switching to
another workspace closes every session, running or exited, after asking (see
[Workspace](#workspace-several-folders)); a `--session` window then keeps none of them.

| Task | Keys |
|---|---|
| Show the AI panel with the keys / hide it (the agents keep running) | `f12` |
| Start a session (picker: tool, then New / Continue last / Choose earlier) | `ctrl+f12`, or the `+` of its tab bar, or `/ai` (picker) or `/ai <tool>` |
| Next / previous session | `f7` / `shift+f7` |
| See what changed while the session ran | `f6` (list at the top of its tab), `enter` opens a file as a diff in the editor |
| Keep / revert the selected change (list or diff tab) | `f5` / `shift+f5` |
| Back to the editor (panel stays) | `f3` |
| Panel width while it has the keys | `ctrl+f8` / `ctrl+f9`, or drag its separator |
| Rename / close a session | palette: AI: Rename Session…, AI: Close Session (or the tab's `x`) |

A focused session takes every key except `f1`, `f3`, `f6`, `f7`, `shift+f7`, `f12`, `ctrl+f12`,
`ctrl+f8`, `ctrl+f9` and ``ctrl+` ``; `ctrl+w`, `ctrl+n` and `f5` reach the agent (`f6` opens the changes list). Continue last and
Choose earlier use the tool's own resume (claude `--continue` / `--resume`, codex `resume --last` /
`resume`: `continue_args` and `resume_args` in `ai_tools`); vr-ide never reads the tools' history.
A user file with the older `ai_presets` list still works: it is read as `ai_tools` (set only one
of the two keys), and its entries need `continue_args` / `resume_args` for Continue last / Choose
earlier.
An agent that exits keeps its screen (`shift+pageup` / `shift+pagedown` scroll it back) and shows
the exit line below it; `enter` starts it again with its continue arguments. With
`vr-ide --session` the panel comes back as it was (shown, width, sessions and their names), each
session started with its continue arguments and sent no input; a session whose tool is no longer
in `ai_tools` or on `PATH`, or whose folder is no longer a workspace folder, is not reopened (one
notice each). While the panel shows, a terminal set to the right sits at the bottom of
the editor column. The tools are started directly (no shell), found only in the absolute
`PATH` entries, and get no `VR_IDE_*` variables.

#### Accounts

claude and codex sessions can each run on their own account, side by side. vr-ide keeps one
config folder per account, `~/.local/state/vr-ide/accounts/<name>/` (private to you, 0700),
and starts a session on it with `CLAUDE_CONFIG_DIR` (claude) or `CODEX_HOME` (codex) set to that
folder, for that session only; `default` is the tool's own config (no variable). The picker lists
each tool's accounts under it.

- Add: `+ Add account…`, type a name (`a-z 0-9 - _`, up to 32, unique across tools), `enter`: the
  folder is made and a session starts on it. Sign in once with `/login` in that tab; the tool
  writes its login into the folder. vr-ide never reads, copies or stores tokens.
- Use: pick the account in the picker, or `/ai claude @work` (`tab` after `@` completes the names). The tab shows it (`2 claude@work`);
  a restored session reopens on the same account, or not at all (a notice) when its folder is
  missing or not safe — never on `default`.
- Remove: `del` on the account in the picker. It asks first, and is refused while a session on it
  is open or its login window is showing. The folder is deleted for good (not to the trash):
  the tool's login and settings, and with it the web sign-in the [login window](#login-window)
  kept for the account. Conversation history is shared and stays: only the account's link to it
  is removed (history that could not be shared, see below, lies in the folder and goes with it;
  the question says which applies). The sign-in stays valid at the provider, so run `/logout`
  in a session on that account first if you want it ended.
- History is shared, logins are not: every account of a tool sees the conversations of the
  default account and of each other (`/resume` lists them all). vr-ide makes the account's
  history entry (`projects` for claude, `sessions` for codex) a link to the default account's
  history folder, `${CLAUDE_CONFIG_DIR:-~/.claude}/projects` or `${CODEX_HOME:-~/.codex}/sessions`
  as vr-ide itself was started. An account that already has history gets it moved into that
  folder once, when a session starts on it and no other session uses it; nothing is ever
  replaced. The account stays "not shared" (a warning says why, its files stay where they are)
  when its entry is a link elsewhere or a file, a file exists on both sides, it
  holds links or special files, the two folders are on different file systems, or the shared
  folder is open to others (vr-ide does not change its permissions: run `chmod 700` on it).
  It is tried again on the next start. Removing an account removes only the link.
- A folder that is a symlink, belongs to another user, or is open to others is refused or made
  private (0700) before use. vr-ide switches accounts on its own only at a usage limit (below).

#### Login window

When claude asks you to sign in (`/login`, or the first start on a new account), its sign-in page
opens in a small vr-ide window titled with the account (`vr-ide login - claude@work`), not in
your browser. Each account has its own web data (cookies, storage, cache) in
`~/.local/state/vr-ide/ai-login/claude-<account>/` (private, 0700), so the browser's own claude
login and the other accounts never get in the way, and signing the same account in again later
is quick. When you approve, the window closes and claude continues.

- How it works: vr-ide starts each claude session with `BROWSER` set to a launcher of that
  account, `~/.local/state/vr-ide/ai-login/bin/vr-ide-login.claude.<account>`, a link to the
  vr-ide program (claude runs `$BROWSER <url>` as one program, claude 2.1.332). Only that claude
  session gets it; terminals, ssh sessions and other agents do not.
- The window loads only `https://claude.ai`, `claude.com`, `platform.claude.com`,
  `*.anthropic.com`, the sign-in pages of Google (`accounts.google.com`) and Apple
  (`appleid.apple.com`), and claude's own `http://localhost:<port>` callback. Any other page is
  not loaded; the window title then says `blocked <scheme>://<host>`. Downloads, pop-up windows
  and camera / microphone / location requests are refused. Sign-in by e-mail code works; a
  sign-in link from an e-mail opens in your browser, not in this window, so use the code. If a
  provider refuses to sign in inside an embedded window, close it: claude shows the URL to open
  in your browser and asks for the code.
- Everything else that claude or a program under it opens (a documentation link, `gh auth
  login`, an MCP server's sign-in, a local dev server) goes to your normal browser through
  `xdg-open`, as without vr-ide.
- One window per account. If `/login` is run again while that account's window is still open,
  no second window opens: close the open one first, then run `/login` again (the open window
  belongs to the earlier sign-in and its code no longer reaches claude). It closes itself after
  15 minutes. Closing it by hand is fine: claude
  still shows the URL and the "paste code" prompt.
- No display (an SSH session, no `DISPLAY` / `WAYLAND_DISPLAY`), a build without the window, or
  a vr-ide or state folder path with characters other than `A-Z a-z 0-9 / . _ + -`: no launcher
  is set and claude signs in its own way (it prints the URL; open it anywhere and paste the
  code). For the path case a notice says so once.
- The web sign-in is as private as the account folder: any program running as you could read
  either. Removing the account deletes its web data here; it stays valid at the provider.
- A program running as you could open this window for an account with a real claude sign-in URL
  of its own; the window always names the account in its title: approve only a sign-in you
  started.

#### Usage

A claude session shows how much of its account's usage limit is used, as claude reports it to its
status line (vr-ide starts claude with `--settings` naming vr-ide itself as the status-line
helper; nothing is read over the network or from claude's files). The tab reads
`2 claude@work 63%`: the 5-hour window, or the weekly one as `91%w` when that is the higher, in the
warning colour from 90 %. When the tab bar runs short the percent goes before any tab is folded
into `+N`; a tab at 90 % or more then shows `!` after its number (`2!`), in the warning colour.

- Reaching 90 % of a window gives one notice per window, e.g. `claude@work used 91% of its 5-hour
  limit (resets 15:00)`.
- `/ai usage` lists the active session's account with both windows and their reset times, in
  local time (`Dec 3 15:00` when a day or more away).
- When the tool shows its limit message, one notice says so, with the reset time when shown.
  When no reset time is known (none on screen, none in a report at 100 %), the account counts as
  limited for an hour, unless claude reports it under 100 % before then.

Other tools (codex, gemini, ...) show no usage; `/ai usage` there says `codex does not report
usage`.

#### Switching accounts at a limit

When a claude session shows its limit message, vr-ide moves it to another account: it ends
claude, waits until it is gone, starts it again in the same tab on the next account with
`--resume <conversation id>`, and types one line (`ai_auto_switch_prompt`, default `Continue where
you left off.`) and Enter once claude's input box is back. A notice says so and stays until
dismissed: `switched from claude@work to claude@personal (claude@work limited until 15:00)`
(`claude` alone is the default account). Reaching 100 %
alone switches nothing; only the limit message does.

- The next account is the one after the current in the picker's order (`default`, then the
  names), wrapping. Skipped: accounts limited until a later time, accounts whose history is not
  shared, and accounts vr-ide has not yet seen working — an account counts as working once
  claude has run on it in vr-ide and reported its usage, so sign in and use each account once.
  An account that shows the login screen is taken as not working and the next one is tried.
- Nothing is switched, and a notice says why, when the conversation id is not known yet (claude
  reports it with its usage), or no account is left. When every other account is limited:
  `all claude accounts are limited until 15:00` (the earliest reset of the others); otherwise
  each one with its reason, e.g. `no other claude account can be used now (claude: not used in
  vr-ide yet; claude@home: busy (being removed or set up); claude@team: limited until 15:00)`. The reasons: limited,
  not used in vr-ide yet, history not shared (its warning comes once per account per run),
  busy (being removed or set up), account folder not usable, not logged in. The session stays
  as it is.
- The line is typed only into claude's idle input box: not into a trust, login or permission
  question, and not when you typed, pasted or clicked in the session after the limit. If the
  box does not appear within 30 seconds: `claude@personal is waiting: type to continue`.
- At most three switches per session in 30 minutes; then auto-switch turns itself off for that
  session, with a notice naming `/ai autoswitch on` to turn it back on. A limit in the first minute on the new account counts only once
  claude has reported from it.
- Off: `ai_auto_switch = false` in `settings.toml` for new sessions, or `/ai autoswitch off`
  (`on`) for the active one; its tab then shows `⊘` after the label (`2 claude@work ⊘ 63%`).
- A resumed conversation shows its last lines again; an old limit message among them counts
  only after you (or the typed line) sent a non-empty line to that run; with "Choose earlier" the Enter
  that picks the conversation in its list does not count.

Enter on an exited claude session, and reopening a `--session` window, also resume the tab's own
conversation by its id when claude reported one (the id is kept in the session file); without
an id they use the continue arguments, which with a shared history pick the folder's most
recent conversation of any account and tab. If claude does not find the conversation, the next
Enter falls back to the continue arguments. A run that cannot report (its report files could not
be set up) keeps its id when it ends.

Codex sessions switch the same way, with `codex resume <conversation id>` (never `resume --last`),
and Enter on an exited codex session or a restored tab resumes by that id too. Codex has no status
line for vr-ide, so its id comes from codex's `notify` program: vr-ide starts codex with
`-c notify=["<vr-ide>", "--ai-codex-notify", "<report file>"]`, and after each finished turn codex
runs vr-ide with the turn's details, of which only the conversation id is kept (nothing is read
from codex's files). This replaces your own `notify` setting from `~/.codex/config.toml` for the
sessions vr-ide starts. What follows from it:

- The id is known only once codex finished its first answer in that run; a limit before that
  gives the limit notice and `codex@work: session id not known; not switching accounts`.
- A turn whose text is larger than 128 KiB (a big paste) reports no id: Linux does not let codex
  pass that much to vr-ide in one argument. Any smaller turn before or after it does.
- Enter on an exited codex tab resumes its conversation by id even when you quit before codex
  answered; only codex's own `No saved session found with ID …` drops the id, and the next start
  is then a new conversation (never `resume --last`).
- A codex tab with no id at all (quit before codex's first answer) also starts a new conversation
  on Enter or when its `--session` window reopens, never `resume --last` (which could open another
  tab's conversation), with the notice `codex@work: conversation id not known; started a new
  conversation`.
- Codex does not report usage this way, so a codex tab shows no percent and `/ai usage` has
  nothing for it; limits are seen from codex's limit message only. A codex account counts as
  working once it finished an answer in vr-ide.
- The continue line is not typed into codex (vr-ide does not know codex's input box and
  questions): after the switch a notice says `codex@personal is waiting: type to continue`.
  A sign-in screen is still noticed and the next account tried.
- With codex's sub-agents the id of the last finished turn is kept; should that be a sub-agent's,
  the switch resumes that conversation of the same folder.

The other tools (gemini, aider, cursor-agent) get no limit notice and are not switched.

#### Agent notifications

When an agent rings the terminal bell (BEL) or sends a desktop notification (OSC 9, or OSC 777
`notify`), its tab gets a yellow `◆` and a notice `◆ <tab> (<folder>): <message>  f12 shows it`
appears, also while the panel is hidden (the status bar then shows `◆ <tab>  f12`). `f12` opens
the session that has waited longest; viewing a tab clears its mark and notice. The text comes from
the program, so it is shown with control characters made visible, cut at 200 characters, behind the
tab name, and at most one notice per session per second. The terminal panel's tabs do not mark.

The agent has to send the signal: vr-ide reports itself to the tool as a plain `xterm-256color`
terminal, so a tool that only notifies inside terminals it recognises stays quiet. vr-ide does not
change the tools' configuration; set it yourself if you want the marks. Claude Code: the
`preferredNotifChannel` setting (`terminal_bell` rings BEL when it needs you; `auto` only notifies
in terminals it knows). Codex: the `[tui]` notification settings in `~/.codex/config.toml`
(`notification_method = "bel"` or `"osc9"`). Names change between tool versions; check the tool's
own documentation if the mark does not appear.

**AI changes.** While a session's program runs, vr-ide records which files in its folder change
(`M` modified, `A` created, `D` deleted; `±N` on the tab bar) — by any program except vr-ide's
own editor, so another program's edits in that folder during that time count as the agent's (the
list says so). `f6` lists them; `enter` shows a file as a read-only diff (earlier version against
the file now, with the file's syntax colours). `f5` (Keep) drops the record. `shift+f5` (Revert)
asks first, then writes the earlier version back byte for byte with its permission bits, or moves
a file the agent created to the trash. Both act on the version you were shown: when the file
changed again since then, nothing is kept or reverted and the diff reloads. Revert also refuses
when the file has unsaved edits in the editor, or when a folder now sits at the file's path. A
version Revert could neither keep in place nor prove to be its own is never deleted: the message
names the file it was kept in. The version Revert replaces is not deleted either: it goes to the
trash as `<name>.vr-ide-reverted-…` (one trash entry per Revert), so a program that still had the
file open and writes a moment later writes into that trash copy, not into nothing. Where there is
no trash for the folder, that version stays next to the file under that name and the notice says
where. On file systems without an atomic exchange of two files (NFS, sshfs and other FUSE mounts)
the replaced version gets a second name before it is replaced (a copy where there are no hard
links) and goes to the trash the same way; where neither is possible Revert is refused and the
file stays as it is. The trash works by path, so a file written to in the instant it is
moved there still goes to the trash; the notice then says the trash holds a version you were not
shown. `f5` / `shift+f5` act only in that list or on a diff tab; in a focused session they go to
the agent.

The earlier version is a copy taken when the session starts, only of files that were last written
before that moment and that the ignore rules (`.gitignore`) do not leave out. A file vr-ide first
saw after its change, one over 8 MiB, or one past the limits (256 MiB per session, 512 MiB for all
sessions of all running vr-ide windows, 5 000 files per list, 10 000 watched folders) is still
listed, marked `no earlier` / `too large` / `limit`: Keep only. The list says when it may be
incomplete: while the folder is still being scanned after the session started (a folder not
reached yet is not watched), when the file watcher lost events, and when the folder has more
files or folders than vr-ide follows (200 000 files). Two notices stay until you press `enter`
on them: `Incomplete: scan stopped early` when the program exited before the scan had been
everywhere (what it changed in the folders not reached is not listed), and `Incomplete: files
deleted during scan` when a folder's entries changed, or the folder itself went away, after the
session started and before the scan reached it (a file deleted in that time left no trace). A
file you save in vr-ide's editor and the agent
then rewrites at once is listed `no earlier`: the copy from before your save is not offered, so
Revert cannot undo your save. Copies live in `~/.local/state/vr-ide/ai/` (folder `0700`, files `0600`, also for
secret files such as `.env`) until Keep, Revert or the session's tab closes; what a crash leaves
is removed at the next start. `.git/`, the trash, vr-ide's state and config folders are never
recorded.

"Last written before the session started" is read from the file's change time (ctime), which the
file system sets and a program cannot set back. On a local disk that is this machine's clock. On
NFS and other network file systems it is the server's clock: if the server's clock runs behind
this machine's by more than a moment, a file the agent wrote right after it started can look
older than the session, and its content would be taken as the earlier version. Keep the clocks in
step (NTP) when you run agents in a network folder, or check a diff there before you trust Revert.

### SSH sessions

vr-ide opens SSH sessions with your system `ssh` (OpenSSH), so your `~/.ssh/config`, keys, agent
and known hosts apply as they do in a terminal.

- Open the command window (`F1`) and type `ssh` and a space (or `/ssh `, or pick SSH: Connect to
  Host). The list shows the open sessions first, then recently used lines, then the `Host` names
  of `~/.ssh/config` and the files it includes (patterns such as `*` are left out). Type what you
  would type after `ssh ` in a terminal: `/ssh -i credentials/id_fixecom root@app.fixecom.vn`,
  `/ssh -p 2222 user@host`, `/ssh -J jump host`. The first row is what you typed; under the list
  the window shows `runs: ssh …`, the exact command with relative paths shown resolved. Enter
  starts the highlighted row.
- The line is split like a shell splits it, but no shell runs it: `'…'` keeps everything,
  `"…"` keeps everything but `\"`, `\\`, `` \` `` and `\$`, `\` keeps the next character, and
  `~` / `~/…` at the start of a word is your home folder. `$VAR`, backticks, `*`, `;`, `|` and
  redirections are ordinary characters. A line with an unclosed quote, a trailing `\`, a control
  character, more than 4 KiB or 128 words, an option ssh does not know or no valid destination is
  refused (the notice never repeats what you typed) and nothing is run. An old `user@host:port`
  still works and means `-p port user@host`.
- ssh runs in the workspace folder of the active file (else the active one), so relative paths
  such as `-i credentials/key`, `-F config` or `-o IdentityFile=…` resolve as in a terminal
  opened there; if that folder is gone nothing is started. ssh is found on the absolute entries
  of `PATH` only, and the session gets a `PATH` without empty or relative entries.
- The session starts in a new tab of the terminal panel, titled `ssh <destination>` (the host
  word ssh connects to, found with ssh's own option rules). Host-key questions, passwords and
  passphrases are asked by ssh and answered there. vr-ide does not read, store or log what you
  type into a terminal.
- When the login succeeded the tab takes the whole window: only the last row stays vr-ide's
  (`SSH <destination>` and the key back). Every key goes to the session except the keys a focused
  terminal keeps: ``ctrl+` `` (back to the editor; the session keeps running), `F1`, `ctrl+f7`,
  `ctrl+f8`, `ctrl+f9` (these three change the panel only in the IDE layout; in full screen they
  just say so). `shift+pageup` / `shift+pagedown` scroll back as in any terminal tab. Notices
  raised meanwhile wait for the IDE layout (the last row counts them) and are shown when it is
  back.
- To return to a session: click its tab in the terminal panel (dragging the tab moves it along
  the bar instead), or choose it in the ssh list. Several sessions can be open, one tab each (at
  most 16).
- When ssh ends (logout, a dropped connection, a failed login) the editor comes back and the tab
  shows `[process exited N]`; Enter there runs the same line again in the same folder. Quitting
  vr-ide or closing the tab of a live session asks first.
- Recently used lines (at most 20, recorded after a successful login, or after a clean exit when
  `ssh_fullscreen = false`) are kept with the folder they ran in, in
  `~/.local/state/vr-ide/ssh-recent`, readable by you only (a file others can write is ignored).
  A recent line chosen from the list (or put in the input from it and not edited) runs again in
  that folder, and the window shows `in <folder>`; the same line typed by hand runs in the
  workspace folder of the active file. A long row is cut in the middle so its first option
  and its destination stay visible. Choosing a row whose line makes ssh run a local program
  (`ProxyCommand`, `LocalCommand`, other `…Command`, `PKCS11Provider`, `SecurityKeyProvider`,
  `-I`, bundled or not) or read a config file of its own (`-F` other than `-F none`) only puts
  it in the input with a warning; Enter then runs it. No password or key is ever
  stored, but a remote command you typed is part of the line.

How the login is detected: vr-ide starts ssh as
`ssh -o ControlMaster=no -o PermitLocalCommand=yes -o LocalCommand=<prints a one-time marker>
<your words>`. ssh runs `LocalCommand` on your machine only after a successful login; the marker
is random per start and never shown, so nothing the remote side prints can fake it. vr-ide adds
nothing after your words and passes no other option: host-key checking and everything else stay
as you configured them. The session stays an ordinary panel tab (a notice says why) when

- your words set `LocalCommand`, `PermitLocalCommand` or connection sharing (`ControlMaster`,
  `ControlPath`, `ControlPersist`, `-M`, `-S`, `-O`), or use `-G`, `-V`, `-Q` or `-f` (vr-ide
  then adds none of its options, since ssh keeps the first value it is given),
- your ssh config sets a `LocalCommand` of its own for that host (vr-ide then adds none; it asks
  `ssh -G <your words>` in the same folder, at most 5 s),
- ssh reuses a shared connection (`ControlMaster` / `ControlPath`; such a session runs no
  `LocalCommand`), or
- `ssh_fullscreen = false` is set in `settings.toml` (ssh is then started without the two
  `LocalCommand` options).

The remote side can draw anything inside the session, including something that looks like
vr-ide. The last row (`SSH <target>`) is drawn by vr-ide and cannot be written by the session.

### Git

In a git repository the gutter marks changed lines and the status bar shows the branch with
`↑n ↓n` against its upstream (see `docs/commands.md`). The welcome screen shows the branch too.
With `git_inline_blame = true` in `settings.toml` a dim `Author, 3 days ago • subject` follows the
cursor line once the cursor rests there. `shift+f2` (palette: "Toggle Blame Annotations") shows
or hides a blame column left of the line numbers: author and commit date (UTC) on the first line
of each run of lines from one commit, `Not committed yet` on lines not committed; it applies to
every editor tab for the session, in read and edit mode, follows your edits and is read again on
save. vr-ide only reads the repository: stage, commit, push,
pull and history are done with git in a terminal tab. Recovery copies that older builds wrote
under `~/.local/state/vr-ide/git-recovery` are no longer listed and can be deleted by hand.

### Clipboard

Copy and paste use the system clipboard when a display is reachable (`DISPLAY` or
`WAYLAND_DISPLAY`). Without one (a plain console, SSH), vr-ide copies with OSC 52 instead: the
terminal puts the text on its own computer's clipboard. Paste then returns the last text copied
inside vr-ide, because terminals rarely let programs read the clipboard; paste from other programs
with your terminal's own paste shortcut (bracketed paste is supported).

## Configuration

| File | Purpose |
|---|---|
| `~/.config/vr-ide/settings.toml` | Tab width, spaces vs tabs, theme (`synthwave` by default, `dracula` also paints its own colours; `dark` and `light` share a neutral terminal palette; the terminal panel is black with green text in every theme), hidden/ignored files in the explorer, highlight size limit, terminal scrollback (`scrollback`), panel height, position and width (`terminal_height_percent`, `terminal_position`, `terminal_width_percent`) and whether NUL reaches terminal programs in a terminal without the kitty protocol (`terminal_pass_nul`, off), whether a logged-in SSH session takes the whole window (`ssh_fullscreen`, on), and the inline git blame of the cursor line (`git_inline_blame`, off). Built-in values: `crates/vr-config/defaults/settings.toml` |
| `~/.config/vr-ide/keymap.toml` | Key overrides on top of the defaults; see [docs/keymap.md](docs/keymap.md) |
| `~/.config/vr-ide/languages.toml` | File patterns → language for syntax highlighting; see [Syntax highlighting](#syntax-highlighting). Built-in table: `crates/vr-config/defaults/languages.toml` |

These files are optional; a missing key keeps its default. A bad entry is reported in a startup notice
and the rest of the file still applies. Saving `settings.toml` or `keymap.toml` while vr-ide runs (`/config`,
`/keymap`, or any other editor) applies it at once: only the settings whose value in the file
changed, and the whole keymap. A file with an error then changes nothing (the old values stay) and
the error is shown. Example:

```toml
# ~/.config/vr-ide/settings.toml
tab_width = 2
insert_spaces = false
show_ignored = true
```

```toml
# ~/.config/vr-ide/keymap.toml
"f2" = "file.reload"                     # add a key (one key per binding)
"ctrl+q" = ""                            # remove a built-in binding
```

The key spelling, context tables (`[editor]`, `[explorer]`) and every command name are in
[docs/keymap.md](docs/keymap.md).

### Syntax highlighting

Files are coloured by their language with tree-sitter grammars compiled into the binary, in the
active theme's colours (keywords, strings, comments, functions, types, numbers, tags, attributes
and more). Colours do not depend on read or edit mode, and they stay under the selection and find
matches in synthwave and dracula (dark and light show plain text there, since the terminal's palette is unknown).

- Languages: PHP, Blade, Twig, JavaScript (and JSX), TypeScript, TSX, Vue, HTML, CSS, SCSS, Dart,
  Go, Python, Rust, JSON, YAML, TOML, Markdown, XML (and DTD). Embedded code is coloured in its own language:
  `<script>` / `<style>` in HTML and Vue, HTML around PHP, Markdown fenced code by its info string.
  Every language with its extensions, grammar source and licence: [docs/languages.md](docs/languages.md).
- A file with an unknown extension opens as plain text, with no notice.
- The parse runs in the background: a file shows plain on its first frame and gains colours when
  the parse lands; typing never waits for it.
- `max_highlight_bytes` in `settings.toml` (default 10 MiB): larger files open as plain text with a
  one-line notice. A parse that takes longer than 10 s also leaves the file plain, with a notice.
- `languages.toml` maps file names to languages. Each `[id]` table has `extensions` (without the
  dot; the longest match wins), `files` (exact names or `*` / `?` globs; they win over
  extensions), `aliases` (names used in embedded code) and `grammar` (defaults to the id). Your
  file overrides the built-in table field by field, and a new table adds a language that uses a
  compiled-in grammar. A language whose grammar is not in the build is named in a startup notice
  and its files open plain.

```toml
# ~/.config/vr-ide/languages.toml
[javascript]
extensions = ["js", "mjs", "cjs", "jsx", "es6"]   # also colour .es6 files as JavaScript

[php]
files = ["*.ctp"]                                  # CakePHP templates
```

Environment overrides:

- `VR_IDE_CONFIG_DIR`: replaces `~/.config/vr-ide` (the config folder, `XDG_CONFIG_HOME` is honoured otherwise).
- `VR_IDE_STATE_DIR`: replaces `~/.local/state/vr-ide` (state folder, `XDG_STATE_HOME` is honoured otherwise).

## Troubleshooting

- **A key does nothing.** The default keys work in every terminal, but your window manager or
  terminal may take one (rebind it in `keymap.toml`). If your own keymap binds a key the terminal
  cannot send, vr-ide shows a notice at start-up; use the palette (`F1`) or rebind it.
- **Typing does nothing and "Read mode: space i to edit" appears.** The tab is in read mode;
  press `space` then `i` to edit. (`space` followed by another key shows "space i: edit · space
  space: commands" and does nothing else.)
- **Backspace toggles hidden files.** Your terminal sends Backspace as `ctrl+h` (byte 0x08);
  GNOME Terminal sends 0x7f. Set the terminal's Backspace to send ASCII DEL (0x7f), or rebind
  `explorer.toggle_hidden` in `keymap.toml`.
- **`ctrl+[` does not outdent.** Terminals send it as `Esc`; use `shift+tab`.
- **Copy does not reach the clipboard over SSH.** vr-ide uses OSC 52 there, which your local
  terminal must allow (it is off by default in some, for example tmux needs `set -g set-clipboard
  on`). Inside vr-ide, paste still returns what you last copied in it.
- **A notice about leftover files.** After an interrupted or refused save or move, files named
  `.vr-ide-save-*.tmp`, `.vr-ide-conflict-*` or `.*.vr-tmp-*` may remain next to the original. vr-ide
  never deletes them because they can hold the only copy of someone's data: check them and remove
  them yourself.
- **I lost unsaved text after a crash.** See [Recovery](#recovery).
- **`vr-ide: path does not exist` / `usage: vr-ide [<dir>|<file>]`.** The argument must be an
  existing file or directory, and there can be only one.
- **`cargo: command not found`.** Add `~/.cargo/bin` to your `PATH` (see [Requirements](#requirements)).

## Recovery

If vr-ide panics, it restores the terminal and writes every unsaved buffer to
`~/.local/state/vr-ide/recovery/<timestamp>-<path>` (under `VR_IDE_STATE_DIR` when set). Choosing
Discard on a close or quit prompt also keeps a copy there. Recovery files
are never written into your project.

## Safety model

- Saves are atomic: the new content is written beside the file and renamed over it.
- vr-ide never overwrites a version somebody else wrote. If the file changed on disk after you
  opened it, the save stops and asks; overwriting is an explicit choice (`file.save_overwrite`).
  On Linux filesystems with atomic exchange (`renameat2(RENAME_EXCHANGE)`), a concurrent writer's
  version is never silently replaced or deleted. Elsewhere (non-Linux, some FUSE and network
  filesystems) a tiny window between the last check and the rename remains.
- If another program writes the file in the instant a save (or a project replace) is swapping it
  in, vr-ide puts the other version back or keeps it as `.vr-ide-conflict-*` next to the file, and
  the error names where. A save simply refused because the file changed earlier leaves no copy.
- Deleting in the explorer asks first and moves the item to the system trash.
- Explorer copy, move and rename never overwrite an existing item.

## Terminal notes

- A terminal with the kitty keyboard protocol (kitty, WezTerm, foot, Ghostty, recent Alacritty and
  others) is not required: the default keys are all `ctrl` keys and `F` keys (alone, with `ctrl` or
  with `shift`), which any terminal can send.
- A key you add that a terminal without it cannot send is listed in a startup notice; `F1` always
  opens the palette, from which every command can be run.

## Scope and roadmap

Current scope (M1-M3): editor core (undo/redo, selection, long lines, large files), tabs, file
explorer with create/rename/copy/move/delete, command palette, go to file, find and replace in the
file, search and replace across the project, keymap and settings files.

Roadmap:

- M4: syntax highlighting (tree-sitter)
- M5: integrated terminal (in progress: the terminal panel works, see [Terminal](#terminal))
- M6: AI agent console
- M7: git (gutter signs, inline blame and the status bar branch, see [Git](#git))
- M8: LSP (completion, hover, definition, references, diagnostics, rename, format, code actions)

## Benchmark

`scripts/bench-startup.sh [project-dir]` builds a release binary and prints time to first frame
(including pty setup) and max RSS for an empty project, then for a generated 200 KB Rust file (first
frame, RSS at the first frame, max RSS after the colours landed). A third case leaves the
empty project (welcome dashboard with its monitor) open for 4 s and reports max RSS.

Measured on a release build after M4 (syntax highlighting) with a 5 000-file project (50 folders x 100 files; the script takes any folder, this one is generated by hand), median of 5 runs:

| Metric | Result | Target |
|---|---|---|
| First frame | 23 ms | < 100 ms |
| Max RSS | 5 MB | < 60 MB |

Opening a single 100 MB text file (`vr-ide <file>`): first frame in 145 ms, max RSS 210 MB.

With syntax highlighting (M4), a generated 200 KB Rust file opened with `vr-ide <file>`: first frame
23 ms (the parse runs in the background), max RSS 6 MB at the first frame and 17-18 MB after the
colours landed. Release binary: 18.7 MiB stripped (the default), 22.5 MiB unstripped; the
grammars account for about 14.7 MB of it.
