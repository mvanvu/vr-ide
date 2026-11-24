---
profiled: 2026-11-24
status: greenfield — commands provisional until T1 (scaffold) is done, then re-profiled
stack: rust (cargo workspace)
ui: tui            # terminal UI (ratatui); web/mobile UI packs do not apply
mobile_platform: none
frontend_arch: none
ui_widths: none
ui_sizes: "terminal 80x24, 120x40, 40x12 (ratatui TestBackend snapshots)"
mockup_dir: none
tests: cargo test
lint: cargo clippy
typecheck: cargo check
max_agents: 5
max_verifiers: 4
test_isolation: per-crate   # crates compile separately; concurrent tasks only in different crates
test_logins: none
design_taste: "minimal, nvim-like density; no decoration that costs space; a colourful themed look (Dracula / Homebrew style) with its own background, not the terminal's default colours (user, 2026-11-24 20:40)"
---

# Project profile — vr-ide

## Commands
| Purpose | Command | Status |
|---|---|---|
| build | `cargo build --workspace` | verified (T1) |
| test (one crate) | `cargo test -p <crate>` | verified (T1) |
| test (one file/module) | `cargo test -p <crate> <module>::` | verified (T1) |
| test (full, sign-off only, heavy) | `$TEAM_BIN/run-heavy.sh cargo test --workspace -j 4` | provisional |
| lint changed crate | `cargo clippy -p <crate> --all-targets -- -D warnings` | verified (T1) |
| format changed files | `rustfmt --edition 2024 <files>` | verified (T1: cargo fmt --all) |
| guard | `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` | verified (T1) |
| release build (heavy) | `$TEAM_BIN/run-heavy.sh cargo build --release -p vr-ide` | verified (T1) |
| install | `cargo install --path crates/vr-ide` | provisional |
| bench (startup/RSS) | `scripts/bench-startup.sh` (added by T1) | verified (T1) |

Toolchain: rustup stable (1.100.0 on 2026-11-24); `PATH` needs `$HOME/.cargo/bin`.

## Conventions
- Code, comments, docs in English. Rust edition 2024. `thiserror` in library crates, `anyhow` only in the binary.
- No blocking I/O on the UI thread; background work talks to the app through channels.
- Rendering tests: ratatui `TestBackend` + `insta` snapshots.

## Guard map
| Changed paths | Run |
|---|---|
| `crates/<c>/**` | `cargo test -p <c>` + `cargo clippy -p <c> --all-targets -- -D warnings` |
| `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` | full guard + `cargo build --workspace` |
