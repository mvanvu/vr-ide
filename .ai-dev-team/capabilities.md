# Capabilities — vr-ide

Greenfield: nothing reusable yet. Developers append one line per reusable piece they add
(`` `path` — what it does ``), grouped by crate.

## vr-core
## vr-config
- `crates/vr-config/src/keymap.rs` — `Keymap::load(user) -> (Keymap, warnings)`, `command_for` (editor focus), `command_for_in(Context, chord)` (context then global; Terminal no fallback, D8), `keys_for(cmd)`, `bindings()` iterator (docs/palette), `validate(known)`; `RESERVED_COMMANDS`; defaults in `defaults/keymap.toml`
- `crates/vr-config/src/keys.rs` — `KeyChord{code,mods}` (crossterm-free), `FromStr`/`Display` (`ctrl+shift+alt+<key>`), `KeyChord::new` lowercases letters (use it when converting terminal events)
- `crates/vr-config/src/settings.rs` — `Settings::load(user) -> (Settings, warnings)` per-key override, `Settings::default()`, `AiPreset{name,command,args}`
- `crates/vr-config/src/paths.rs` — `config_dir()`/`state_dir()` with `VR_IDE_CONFIG_DIR`/`VR_IDE_STATE_DIR` override
## vr-fs
- `crates/vr-fs/src/tree.rs` — `Tree::new(root, show_hidden, show_ignored).children(dir)` lazy one-dir listing, dirs first, `Node{path,kind,ignored,depth}`, `NodeKind::Symlink(SymlinkTarget::{File,Dir,Loop,Broken})`, `set_show_hidden/ignored`
- `crates/vr-fs/src/ops.rs` — `create_file` (makes parents), `create_dir`, `rename`, `copy` (recursive, links copied as links), `move_to(from, dest_dir) -> new path`, `trash(paths)`; never overwrite; `FsOpError`; `Trash` trait with `SystemTrash` and `FakeTrash::new(holding_dir).trashed()` for UI tests
- `crates/vr-fs/src/file_index.rs` — `FileIndex::build(root, tx) -> JoinHandle` sends `IndexMsg::{Progress,Ready,Error}` (drop rx to cancel); `query(q, limit)` basename-first nucleo ranking, smart case, char indices into the relative path; `from_paths` for tests
## vr-search
## vr-ide (app)
- `crates/vr-ide/src/cli.rs` — `cli::resolve(args, cwd) -> Launch{root, open_file}` (D5 launch rules)
- `scripts/bench-startup.sh` — release first-frame ms + max RSS under a pty
