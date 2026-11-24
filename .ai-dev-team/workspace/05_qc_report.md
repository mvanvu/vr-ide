
## QC T1 — QC_PASS
Build: release binary sha1 4510ea438aac (no git commit yet; built 2026-11-24 11:27). Harness: `qa-artifacts/T1/pty.py` (python3 -I pty, 24x300 winsize; newlines/wrap lost in TEXT, so the "vr-ide—<root>" and "open:<file>" lines appear joined). Temp repos/dirs created and removed (non-git case lived in the scratchpad because qa-artifacts sits inside the vr-ide git repo).

| AC | Status | Evidence |
|---|---|---|
| AC-1 no args | pass | cwd plain dir: TEXT `vr-ide—<cwd>`; alt screen entered+left, raw during run (ICANON off), after ctrl+q: EXIT 0, canon/echo restored |
| AC-2 file in git repo | pass | file in git repo (abs and relative `a.rs` from src/): `vr-ide—…/work/repo` + `open:…/repo/src/a.rs`; EXIT 0 on ctrl+q |
| AC-2 file outside git | pass | `vr-ide—…/t1nogit/d` + `open:…/d/b.txt` (root = parent dir); dir arg outside git -> root = that dir |
| AC-2 nonexistent | pass | `vr-ide: path does not exist: <path>` on stderr, rc=2, no alt screen entered, terminal canon+echo on |
| Bench | pass | `scripts/bench-startup.sh` x2: `first_frame_ms=23/24`, `max_rss_mb=2` (targets <100/<60) |
| cargo install | pass | `cargo install --path crates/vr-ide --root …/T1/install` -> Installed vr-ide v0.1.0; ~/.cargo/bin untouched; install dir removed after |
| `cargo test -p vr-ide cli` | pass | `cli_resolves_root ... ok; 1 passed` |

Notes: cargo prints 4 `unused dependency` manifest warnings (expected, deps pre-listed for T2-T10). Without ctrl+q the app keeps running (expected).

## Rule check T1
| Rule | Result | Evidence |
|---|---|---|
| AC-1 | pass | pty run, no args |
| AC-2 | pass | git/non-git/nonexistent pty runs |
| D5 | pass | no args = cwd; path arg resolved as above |
| C3 | pass | bench prints first_frame_ms and max_rss_mb, within targets |
| TOOL (install) | pass | scratch-root cargo install OK |
