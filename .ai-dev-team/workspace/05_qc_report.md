
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

## QC T7 — vr-fs tree, file ops (trash), fuzzy file index
Engine: native Rust probe `qa-artifacts/T7/probe` (path dep on crates/vr-fs, release build, own target dir, own `git init` fixtures; fixtures and target removed afterwards, BASE-30). No browser (library, no UI). Developer tests not rerun.

| Criterion | Result | Evidence |
|---|---|---|
| AC-8 create/rename/copy/move | PASS | create_file/dir, AlreadyExists keeps content, rename file+dir, recursive copy keeps source, move_to returns new path and removes source, move into itself -> IntoItself and dir intact, rename missing -> error: 13/13 PASS lines |
| AC-8 trash layout | PASS | under temp XDG_DATA_HOME (same mount as project): `Trash/files/{CtlCopy,renamed.rs}`, `Trash/info/{CtlCopy,renamed.rs}.trashinfo`; docs entries 3 -> 1; trashinfo has `[Trash Info]`, absolute `Path=`, `DeletionDate=`; dir content preserved; name collision -> `renamed.rs.2`. User trash untouched |
| AC-9 tree vs git | PASS | fixture: root .gitignore (dir, glob, anchored), `.git/info/exclude`, nested `sub/.gitignore` with negation (`!keep.tmp`) and nested anchored `/vendor/`, node_modules, unignored root `vendor/`, symlink. Tree visible files (show_hidden true and false) == `git ls-files -co --exclude-standard` exactly (extra=[], missing=[]); `git check-ignore -v` agrees on negation/nested rules; root `vendor/` (not ignored by git) stays visible, `sub/deep/vendor` hidden. show_ignored=true shows node_modules and flags build, node_modules, root.log, secret.txt as ignored |
| AC-9 file index | PASS | index set == git set (extra=[]) in fixture; 49 501 files on the big tree == `git ls-files -co` 49 501; node_modules/vendor absent; query `ordctl` -> `src/Controller/OrderController.php` |
| Review Focus 4 perf (49 500 files + 15 000 ignored in node_modules/vendor) | PASS | index `Ready` 74.6 ms (release); peak RSS 7.5 MB (VmHWM, 2.8 MB before); root listing 3.7-4.0 ms for 2 050 nodes (< 50 ms); `children(node_modules)` with show_ignored=false 0.1 ms; fuzzy query 2-2.5 ms |

Observations (not defects):
- Index omits symlinks to directories (`srclink` is in `git ls-files`, not in the index): consistent with "symlinks not followed", file-only go-to-file.
- Empty-after-ignore dir `logs/` (only `*.log` inside) stays visible in the tree: correct, the dir itself is not matched by any rule.
- Release-build timings; debug build will be slower.

## Rule check T7
| Rule | Status | Evidence |
|---|---|---|
| QC-evidence | pass | own probe output above, not developer tests |
| AC-8 | pass | 13 disk-state checks + freedesktop trash layout |
| AC-9 | pass | tree and index equal git's non-ignored set |
| Review Focus 4 | pass | 74.6 ms / 7.5 MB / 3.7 ms |
| BASE-15/BASE-18 | pass | no background tasks, no sub-agents |
| BASE-17 | pass | probe wrote only under qa-artifacts/T7 and removed it; real trash not touched |
| BASE-24 | pass | no git index changes in vr-ide (fixture repos were separate and deleted) |
| BASE-30 | pass | fixtures, target, Cargo.lock removed; only probe source remains |
n-a: remaining QC rules (no UI, no API contracts, no DB)

QC_PASS T7

## QC T3 — vr-config keymap + settings (AC-5)
Engine: Rust probe crate (`qa-artifacts/T3/probe`, path dep on vr-config, own target dir, `[workspace]` isolated), real files under a temp `VR_IDE_CONFIG_DIR` (removed afterwards, verified). Run: `cargo run --manifest-path .../probe/Cargo.toml` -> 64 checks, FAILS=0. Developer tests not rerun.
| Criterion | Evidence | Result |
|---|---|---|
| AC-5a `ctrl+s = "file.save"` (unquoted) + rebinding | resolves; ctrl+s -> file.save_as and ctrl+shift+s -> file.save after user override; extra user key ctrl+k resolves; 0 warnings | pass |
| AC-5b unknown command | `keymap: ctrl+j is bound to unknown command "nope.nothing" (global)` (names key + command), no panic, rest of file applies; default keymap validates clean against all 47 registry names | pass |
| AC-5c malformed keymap.toml / settings.toml | 1 warning each (with line number, "built-in defaults kept"), keymap == defaults, settings == Settings::default() (tab_width 4); extra: wrong-type value warns, default kept, other key applied | pass |
| AC-5d default keymap vs registry | every registry command VS Code binds by default has a binding and non-empty `keys_for`; 36 D10 chord->command pairs checked in `keys_for`; contexts (f2 explorer, ctrl+c editor vs explorer) correct. Unbound by design (VS Code has no default): file.reload, explorer.reveal/new_folder/move/toggle_ignored/toggle_hidden | pass |
| AC-5e terminal context | 63 chords (all bindings in every context + extras like a, enter, ctrl+c, ctrl+q, f1): none resolves in Terminal except alt+q -> focus.editor; alt+q not resolved in editor; user global binding does not leak into terminal | pass |
Note (not a defect): `Keymap::load` does not itself warn on unknown commands; the caller must run `validate(&registry_names)` (as the contract intends) — T5 must call it at startup.

## Rule check T3
| Rule | Status | Evidence |
|---|---|---|
| AC-5 | pass | table above |
| BASE-1/BASE-24 | pass | no crate files or git index touched; only qa-artifacts/T3 probe |
| BASE-14/15 | pass | probe ran in foreground (<10 s), no background tasks, no kill |
| BASE-17 | pass | temp config dir removed; `ls /tmp` shows no t3probe leftovers |
| COST-13 | pass | throwaway probe in qa-artifacts/T3 |
n-a: no browser/UI/DB in this task
