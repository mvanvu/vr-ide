# vr-ide

A lightweight, non-modal terminal IDE written in Rust: no modes, VS Code's default keys, and a
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
  foot, Ghostty, recent Alacritty and others) is recommended. Without it vr-ide still works, but a
  few default chords cannot be sent (for example `ctrl+shift+p`); `F1` opens the command palette
  there and every command can be run from it. See [Terminal notes](#terminal-notes).

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
  typing does nothing. Press `ctrl+i` to edit (`EDIT`) and `esc` to go back to reading. A new file
  from `ctrl+n` opens ready to type.
- The **project root** is the folder the explorer shows and the one project search, "Go to File" and
  relative paths in prompts refer to.
- With no argument it is the current directory; with a directory it is that directory (the path
  is resolved, so `a/../a/b` works).
- With a file it is the nearest parent folder that contains `.git`, walking up from the file's
  folder; if there is none, the file's own folder.
- vr-ide takes at most one argument and has no options: `vr-ide --help` or `--version` are read as
  paths and fail. A missing path or a second argument prints a message and exits with status 2.
- It needs an interactive terminal; it cannot run with its input or output redirected.

Press `ctrl+shift+p` (or `F1`) to open the command palette, which lists every command with its
key. Press `ctrl+q` to quit. If any file has unsaved changes, vr-ide asks first (see
[Quit guard](#quit-guard)).

## Using vr-ide

The keys below are VS Code's defaults on Linux. The complete list, with command names and the keys
that cannot be changed, is in [docs/keymap.md](docs/keymap.md); every key can be overridden
([Configuration](#configuration)). Commands without a default key can be run from the palette.

### Files and tabs

| Task | Keys |
|---|---|
| Open a file by path (relative to the project root) | `ctrl+o` |
| Go to File (fuzzy search by name) | `ctrl+p` |
| New file (created on disk at once; there are no untitled buffers) | `ctrl+n` |
| Save | `ctrl+s` |
| Save As (a new file name, relative to the project root) | `ctrl+shift+s` |
| Close the tab | `ctrl+w` or `ctrl+f4` |
| Next / previous tab | `ctrl+pagedown` / `ctrl+pageup` |
| Command palette | `ctrl+shift+p` or `F1` |

Closing a tab with unsaved changes asks: Save, Discard or Cancel. Discard keeps a copy of your
changes in the [recovery folder](#recovery).

### Editing

Files open in read mode, so a stray key never changes them. `ctrl+i` switches the tab to edit mode
and `esc` back to read mode (it also clears the selection). The tab returns to read mode whenever
the editor gets the keys on it again: switching tabs, coming back from the explorer, the search
panel or a dialog, or closing the palette without running a command. In read mode, typing and the
editing keys below show the hint "Read mode — ctrl+i to edit"; moving, selecting, copying, find,
Go to Line and save work as usual. The status bar shows `READ` or `EDIT`, and where the terminal
supports it the cursor is a block in read mode and a bar in edit mode. Project replace and reloads
from disk still update open files in either mode.

| Task | Keys |
|---|---|
| Edit mode / read mode | `ctrl+i` / `esc` |
| Undo / redo | `ctrl+z` / `ctrl+shift+z` or `ctrl+y` |
| Copy / cut / paste | `ctrl+c` / `ctrl+x` / `ctrl+v` |
| Select all | `ctrl+a` |
| Extend the selection | hold `shift` with the arrows, `home`, `end`, `pageup`, `pagedown` |
| Move by word / to start or end of the file | `ctrl+left` `ctrl+right` / `ctrl+home` `ctrl+end` |
| Delete line / duplicate line | `ctrl+shift+k` / `shift+alt+up` or `shift+alt+down` |
| Move line up / down | `alt+up` / `alt+down` |
| Indent / outdent | `ctrl+]` / `ctrl+[` or `shift+tab` |
| Toggle line comment | `ctrl+/` |
| Go to Line | `ctrl+g` |

Go to Line asks for `line` or `line:column` (1-based, for example `42` or `42:7`).

Most terminals send `ctrl+[` as `Esc`; use `shift+tab` to outdent there.

### Find and replace in the file

| Task | Keys |
|---|---|
| Find | `ctrl+f` |
| Replace | `ctrl+h` |
| Next / previous match | `enter` / `shift+enter` in the find bar, or `f3` / `shift+f3` |
| Replace one / replace all (edit mode only; find works in read mode) | `ctrl+shift+1` / `ctrl+alt+enter` (or `enter` in the replace field to replace one) |
| Match case / whole word / regex | `alt+c` / `alt+w` / `alt+r` |
| Switch between the find and replace fields | `tab` / `shift+tab` |
| Close the bar | `esc` |

### Project search and replace

| Task | Keys |
|---|---|
| Search in Project | `ctrl+shift+f` |
| Replace in Project | `ctrl+shift+h` |
| Next / previous result | `f4` / `shift+f4` |

In the panel, type in the search field and press `enter` to search; results stream in as the files
are scanned. `alt+c`, `alt+w`, `alt+r` toggle case, whole word and regex. `down` moves into the
results; `enter` on a result opens it. Files matched by `.gitignore` are not searched; hidden
files are. `esc` leaves the panel.

Replacing across files is always previewed first:

1. Press `ctrl+shift+h`, fill in the search and the replace field.
2. Press `enter` in the replace field: the panel shows a preview per file. Nothing is written yet.
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
| Show / hide the explorer (focus stays where it is) | `ctrl+b` |
| Focus the explorer | `ctrl+shift+e` |
| Cycle focus between the parts | `f6` |
| Back to the editor | `esc` |
| Move, expand, collapse | `up` `down` `pageup` `pagedown` `home` `end`; `right` expands, `left` collapses or goes to the parent |
| Open a file / toggle a folder | `enter` |
| New file | `ctrl+n` |
| Rename | `f2` |
| Delete (asks first, moves to the system trash) | `delete` |
| Copy / cut / paste | `ctrl+c` / `ctrl+x` / `ctrl+v` |

Commands from the palette (no default key): `Explorer: New Folder`, `Explorer: Move` (asks for the
destination folder, relative to the project root), `Explorer: Reveal Active File` (expands the tree
down to the file in the active tab), `Explorer: Toggle Hidden Files` and `Explorer: Toggle Ignored
Files`.

By default dotfiles are shown and paths matched by `.gitignore` are hidden. Copy, move and rename
never overwrite an existing item. Mouse clicks and the wheel work in the explorer.

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
replace is still running, `ctrl+q` asks Quit anyway or Cancel (Cancel is the default).

### Clipboard

Copy and paste use the system clipboard when a display is reachable (`DISPLAY` or
`WAYLAND_DISPLAY`). Without one (a plain console, SSH), vr-ide copies with OSC 52 instead: the
terminal puts the text on its own computer's clipboard. Paste then returns the last text copied
inside vr-ide, because terminals rarely let programs read the clipboard; paste from other programs
with your terminal's own paste shortcut (bracketed paste is supported).

## Configuration

| File | Purpose |
|---|---|
| `~/.config/vr-ide/settings.toml` | Tab width, spaces vs tabs, theme (`synthwave` by default, `dracula` also paints its own colours; `dark` and `light` use the terminal's colours), hidden/ignored files in the explorer, highlight size limit. Built-in values: `crates/vr-config/defaults/settings.toml` |
| `~/.config/vr-ide/keymap.toml` | Key overrides on top of the VS Code defaults; see [docs/keymap.md](docs/keymap.md) |
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
"ctrl+shift+d" = "edit.duplicate_line"   # add a key
"ctrl+q" = ""                            # remove a built-in binding
"ctrl+alt+r" = "file.reload"             # bind a palette-only command
```

The key spelling, context tables (`[editor]`, `[explorer]`) and every command name are in
[docs/keymap.md](docs/keymap.md).

### Syntax highlighting

Files are coloured by their language with tree-sitter grammars compiled into the binary, in the
active theme's colours (keywords, strings, comments, functions, types, numbers, tags, attributes
and more). Colours do not depend on read or edit mode, and they stay under the selection and find
matches in synthwave and dracula (dark and light show plain text there, since the terminal's palette is unknown).

- Languages: PHP, Blade, Twig, JavaScript (and JSX), TypeScript, TSX, Vue, HTML, CSS, SCSS, Dart,
  Go, Python, Rust, JSON, YAML, TOML, Markdown. Embedded code is coloured in its own language:
  `<script>` / `<style>` in HTML and Vue, HTML around PHP, Markdown fenced code by its info string.
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

- **A key does nothing.** Your terminal probably cannot send it (no keyboard-enhancement support).
  At start-up vr-ide shows a notice listing the keys it cannot receive. Use the palette (`F1`), a
  terminal with the kitty keyboard protocol, or rebind the command in `keymap.toml` to a chord your
  terminal sends. Some chords are also taken by the terminal itself or by your window manager.
- **`ctrl+shift+e` hides the explorer instead of focusing it.** Without the kitty keyboard
  protocol the terminal sends `ctrl+shift+e` as `ctrl+e`, which toggles the explorer (shows and
  focuses it when hidden, hides it when shown). Use `ctrl+e` or `F6` to reach the tree there.
- **Typing does nothing and "Read mode — ctrl+i to edit" appears.** The tab is in read mode;
  press `ctrl+i` (or `tab`, which is what `ctrl+i` arrives as in a terminal without the kitty
  keyboard protocol) to edit.
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
  others) is recommended: it can send every default key, such as `ctrl+shift+p` and `ctrl+alt+enter`.
- Terminals without it cannot send some chords. vr-ide lists the unreachable default keys in a startup
  notice; `F1` always opens the palette, from which every command can be run.

## Scope and roadmap

Current scope (M1-M3): editor core (undo/redo, selection, long lines, large files), tabs, file
explorer with create/rename/copy/move/delete, command palette, go to file, find and replace in the
file, search and replace across the project, keymap and settings files.

Roadmap:

- M4: syntax highlighting (tree-sitter)
- M5: integrated terminal
- M6: AI agent console
- M7: git (gutter signs, git panel, diff view, hunk staging)
- M8: LSP (completion, hover, definition, references, diagnostics, rename, format, code actions)

## Benchmark

`scripts/bench-startup.sh [project-dir]` builds a release binary and prints time to first frame
(including pty setup) and max RSS.

Measured on a release build with a 5 000-file project (50 folders x 100 files), median of 5 runs:

| Metric | Result | Target |
|---|---|---|
| First frame | 22 ms | < 100 ms |
| Max RSS | 6 MB | < 60 MB |

Opening a single 100 MB text file (`vr-ide <file>`): first frame in 145 ms, max RSS 210 MB.
