# FR-9 README install and usage guide

Progress
- Read cli.rs, main.rs, clipboard.rs, keymap defaults, docs/keymap.md, search/file/app commands.
- Rewrote README.md: Requirements, Install, Quick start, Using vr-ide (by task), Configuration, Troubleshooting, then Recovery, Safety model, Terminal notes, Scope and roadmap, Benchmark (facts kept).

## Verified (commands run)
- `$TEAM_BIN/run-heavy.sh cargo install --path crates/vr-ide --root $TEAM_WS/qa-artifacts/FR-9/root` ->
  `Installing .../root/bin/vr-ide` / `Installed package vr-ide v0.1.0`; binary lands in `<root>/bin`.
- `cargo uninstall --root <root> vr-ide` -> `Removing .../root/bin/vr-ide`.
- `target/release/vr-ide` exists after a release build (build-without-install path; `cargo build --release` itself was covered by the install build, same profile).
- No flags: `vr-ide --help` / `--version` -> `vr-ide: path does not exist: .../--help`, exit 2 (documented).
- `vr-ide a b` -> `vr-ide: usage: vr-ide [<dir>|<file>]`, exit 2; `/nonexistent` -> path error, exit 2.
- No tty (`</dev/null`) -> `Error: No such device or address`, exit 1 (documented as needs an interactive terminal).
- Root resolution under a pty with VR_IDE_EXIT_AFTER_FIRST_FRAME: no arg -> title `vr-ide - /tmp` (cwd); dir arg -> title shows that dir; file arg inside this git repo -> title shows the repo root `/home/rainy/DEV/vr-ide` (git root rule).
- git remote origin is https://github.com/mvanvu/vr-ide.git; rust-toolchain.toml channel = stable, edition 2024.
- Keys checked against docs/keymap.md and crates/vr-config/defaults/keymap.toml; dialogs (Save/Discard/Cancel, Save all/Discard/Cancel, Reload/Keep mine, Replace/Cancel with Cancel default), Go to Line `line[:col]`, Open/Save As relative to project folder, OSC 52 / DISPLAY|WAYLAND_DISPLAY logic, leftover names (`.vr-ide-save-*.tmp`, `.vr-ide-conflict-*`, `.*.vr-tmp-*`) read from code.
- Not run: interactive key presses (no tty driver in this task); keys taken from code and keymap defaults. The tmux `set-clipboard` hint is general terminal knowledge, not vr-ide code.

## Rule compliance
| Rule | Evidence |
|---|---|
| BASE-1 | Only README.md changed (plus this notes file and qa-artifacts) |
| BASE-4 | install/uninstall/CLI-error outputs above |
| BASE-6 | README in English |
| BASE-19 | notes created at start |
| BASE-24 | no git add/commit |
| BASE-32 | D10 VS Code keys; existing README facts kept |
| COST | one install build via run-heavy.sh; nothing left running |
| AC-1, AC-2 | start-up and root resolution documented as verified above |

n-a: all other IDs (docs only, no code, no tests)
- qp1: External changes bullets in README.md replaced with the reviewer's wording (file.rs:143-150); nothing else changed.
