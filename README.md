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
- A file opens in **read mode** (status bar `READ`): you can move, select, copy, find and save, but
  typing does nothing. Press `space` then `i` to edit (`EDIT`) and `esc` to go back to reading;
  `space` `space` opens the command palette and `tab` moves the focus to the next part. A new file
  from `ctrl+n` opens ready to type.
- With no file open, the welcome dashboard shows the project, the keys to start with and, on Linux,
  a live resource monitor (CPU with history and per core, RAM, swap, load, uptime, vr-ide's own CPU
  and memory). It is sampled once a second only while it is on screen.
- The **project root** is the folder the explorer shows and the one project search, "Go to File" and
  relative paths in prompts refer to.
- With no argument it is the current directory; with a directory it is that directory (the path
  is resolved, so `a/../a/b` works).
- With a file it is the nearest parent folder that contains `.git`, walking up from the file's
  folder; if there is none, the file's own folder.
- vr-ide takes at most one argument and has no options: `vr-ide --help` or `--version` are read as
  paths and fail. A missing path or a second argument prints a message and exits with status 2.
- It needs an interactive terminal; it cannot run with its input or output redirected.

Press `F1` to open the command palette, which lists every command with its
key. Press `ctrl+q` to quit. If any file has unsaved changes, vr-ide asks first (see
[Quit guard](#quit-guard)).

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

### Files and tabs

| Task | Keys |
|---|---|
| Open a file by path (relative to the project root) | `ctrl+o` |
| Go to File (fuzzy search by name) | `ctrl+p` |
| New file (created on disk at once; there are no untitled buffers) | `ctrl+n` |
| Save | `ctrl+s` |
| Save As (a new file name, relative to the project root) | palette: Save As |
| Reload the file from disk | palette: Reload File from Disk |
| Close the tab | `ctrl+w` |
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
| Next / previous match or project result | `f3` / `shift+f3` |
| Replace in the box's scope (File: switches the tab to edit mode; find works in read mode) | `ctrl+r` |
| Replace one / replace all in the file (the box shows "enter one · ctrl+a all") | `enter` / `ctrl+a` in the replace field |
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
destination folder, relative to the project root) are palette only.

By default dotfiles are shown and paths matched by `.gitignore` are hidden. Copy, move and rename
never overwrite an existing item. Mouse clicks and the wheel work in the explorer.

A right click on a row selects it and opens a small menu at the pointer with the same actions as
the keys above: New… (File / Folder), Rename…, Copy, Cut, Paste and Delete, each with its key on
the right (taken from your keymap). Actions that cannot apply are dimmed: Paste while nothing is
copied or cut; Rename, Copy, Cut and Delete on the project folder row. A right click below the last
row targets the project folder (New, Paste); one on a row of a multi-selection keeps the selection.
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
unset or does not start) in the project root with `TERM=xterm-256color`. Press it again in the
terminal to give the keys back to the editor (the panel stays shown); from the editor or explorer it
focuses a shown panel. `shift+f12` hides the panel (the shells keep running). In a window so short
that the panel covers the whole editor, ``ctrl+` `` in the terminal hides the panel instead (the
hint says ``ctrl+` hide``). No terminal starts until you open one.

- A focused terminal receives every key, `ctrl+s`, `ctrl+w` and `ctrl+p` included, so `vim`, `htop`
  or `lazygit` work as in any terminal; ``ctrl+` `` is the only key vr-ide keeps. The status bar
  shows `TERM` while the terminal has the keys (read and edit mode do not apply there).
- `ctrl+f12` opens another terminal tab; `ctrl+f4` closes it, `f8` / `shift+f8` move to the next /
  previous one and `ctrl+f8` / `ctrl+f9` grow / shrink the panel. The tab bar shows each terminal's title (set
  by the program, or the shell's name) and `[N]` once it exited. Long titles such as bash's
  `user@host: ~/dir` are cut from the left so the directory stays; when the tabs do not fit, the
  active one stays in view and `+N` counts the others. Click a tab to switch; drag the tab bar to
  resize the panel (`terminal_height_percent` in settings, 40 % by default, at least 5 rows).
- When the program exits the panel says "[process exited N] — Enter to restart, ctrl+w to close"
  (shorter on narrow screens: "exited N · Enter · ctrl+w").
- Notices above the status bar: `Esc` goes to the program while a terminal has the keys, so click
  a notice to dismiss it there.
- `shift+pageup` / `shift+pagedown` and the mouse wheel scroll back (10 000 lines, setting
  `scrollback`); any key or new output returns to the bottom. Programs that use the mouse get it;
  hold `shift` to select text anyway.
- Dragging the mouse selects and copies to the clipboard; `ctrl+shift+v` pastes (bracketed when the
  program asked for it), as does your terminal's own paste.
- Colours: `synthwave` and `dracula` give programs their own 16 ANSI colours and background;
  `dark` and `light` leave your terminal's colours.
- With the kitty keyboard protocol `ctrl+space` is a separate key that reaches the program.
- Without it your terminal sends ``ctrl+` `` as NUL, the same byte as `ctrl+space` and `ctrl+@`.
  vr-ide takes NUL as ``ctrl+` `` everywhere, so it gives the keys back from inside a terminal too,
  and programs never get `ctrl+space`. If a program needs it (Emacs's mark, a shell binding), set
  `terminal_pass_nul = true`: NUL then goes to the program, a click on the editor leaves the terminal, ``ctrl+` ``
  from the editor focuses the panel again, and `shift+f12` from the editor hides it. The tab bar's
  hint shows the way out that works in your setup.

### Clipboard

Copy and paste use the system clipboard when a display is reachable (`DISPLAY` or
`WAYLAND_DISPLAY`). Without one (a plain console, SSH), vr-ide copies with OSC 52 instead: the
terminal puts the text on its own computer's clipboard. Paste then returns the last text copied
inside vr-ide, because terminals rarely let programs read the clipboard; paste from other programs
with your terminal's own paste shortcut (bracketed paste is supported).

## Configuration

| File | Purpose |
|---|---|
| `~/.config/vr-ide/settings.toml` | Tab width, spaces vs tabs, theme (`synthwave` by default, `dracula` also paints its own colours; `dark` and `light` use the terminal's colours), hidden/ignored files in the explorer, highlight size limit, terminal scrollback (`scrollback`), panel height (`terminal_height_percent`) and whether NUL reaches terminal programs in a terminal without the kitty protocol (`terminal_pass_nul`, off). Built-in values: `crates/vr-config/defaults/settings.toml` |
| `~/.config/vr-ide/keymap.toml` | Key overrides on top of the defaults; see [docs/keymap.md](docs/keymap.md) |
| `~/.config/vr-ide/languages.toml` | File patterns → language for syntax highlighting; see [Syntax highlighting](#syntax-highlighting). Built-in table: `crates/vr-config/defaults/languages.toml` |

These files are optional; a missing key keeps its default. A bad entry is reported in a startup notice
and the rest of the file still applies. Example:

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
- M7: git (gutter signs, git panel, diff view, hunk staging)
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
