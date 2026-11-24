# vr-ide

A lightweight, non-modal terminal IDE written in Rust: no modes, VS Code's default keys, and a
small memory footprint. It edits files in a tabbed editor next to a file explorer, with find,
replace, project search and a command palette.

## Build and install

Requires the Rust stable toolchain (install it with [rustup](https://rustup.rs)).

```
cargo build --workspace
cargo install --path crates/vr-ide
```

## Run

```
vr-ide              # open the current directory as the project root
vr-ide <dir>        # open <dir> as the project root
vr-ide <file>       # project root = enclosing git root (else the file's directory); the file is opened
```

Quit with `ctrl+q`. Open the command palette with `ctrl+shift+p` (or `F1`). Every command and key is
listed in [docs/keymap.md](docs/keymap.md).

## Configuration

| File | Purpose |
|---|---|
| `~/.config/vr-ide/settings.toml` | Tab width, spaces vs tabs, theme, hidden/ignored files in the explorer, highlight size limit. Built-in values: `crates/vr-config/defaults/settings.toml` |
| `~/.config/vr-ide/keymap.toml` | Key overrides on top of the VS Code defaults; see [docs/keymap.md](docs/keymap.md) |

Environment overrides:

- `VR_IDE_CONFIG_DIR`: replaces `~/.config/vr-ide` (the config folder, `XDG_CONFIG_HOME` is honoured otherwise).
- `VR_IDE_STATE_DIR`: replaces `~/.local/state/vr-ide` (state folder, `XDG_STATE_HOME` is honoured otherwise).

Both files are optional; a missing key keeps its default. A bad entry is reported in a startup notice
and the rest of the file still applies.

## Recovery

If vr-ide panics, it restores the terminal and writes every unsaved buffer to
`~/.local/state/vr-ide/recovery/<timestamp>-<path>` (under `VR_IDE_STATE_DIR` when set). Recovery files
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
