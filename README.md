# vr-ide

A lightweight, non-modal terminal IDE written in Rust.

## Build and install
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
Quit with `ctrl+q`.

## Benchmark
`scripts/bench-startup.sh [project-dir]` builds a release binary and prints time to first frame and max RSS.
