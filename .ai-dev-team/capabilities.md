# Capabilities — vr-ide

Greenfield: nothing reusable yet. Developers append one line per reusable piece they add
(`` `path` — what it does ``), grouped by crate.

## vr-core
## vr-config
## vr-fs
## vr-search
## vr-ide (app)
- `crates/vr-ide/src/cli.rs` — `cli::resolve(args, cwd) -> Launch{root, open_file}` (D5 launch rules)
- `scripts/bench-startup.sh` — release first-frame ms + max RSS under a pty
