
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

## QC T2 — QC_PASS
Build: vr-core working tree at QC time (path dep, release). Probe: `qa-artifacts/T2/probe/` (own `[workspace]`, no files added to the crate); output `qa-artifacts/T2/run3.txt` (80 PASS, 0 FAIL). Engines: cargo only (library, no UI). Developer tests not rerun; notes show 26 passed + clippy/guard green.
| AC | Status | Evidence |
|---|---|---|
| AC-3 typing/backspace/delete/undo/redo | pass | PHP function typed char by char: text and cursor equal the prefix at every step; newline+indent paste is one undo step; Vietnamese decomposed cluster (e+U+0323+U+0302) removed whole by backspace and delete_forward, precomposed ệ ok; delete_forward at end and backspace at start are no-ops (revision unchanged); undo of 4 groups walks "hello world!!"→"hello world"→"hello"→"" and redo returns; revision == saved value after undoing back to saved text, == initial after full undo; edit after undo clears redo |
| AC-10 base (big transaction) | pass | 1 000 replacements across 10 000 lines match String model, 1 undo restores (revision back, no 2nd group), redo reapplies; mixed grow/insert transaction ok; apply ≈ 0.4–0.7 ms |
| Edit ops | pass | delete/duplicate/move line (first, last, last without trailing newline, single line, CRLF, 2-line selection, no-op move at bounds leaves revision unchanged), indent/outdent (blank lines skipped, tab/2/6-space cases, CRLF), toggle_comment `//` and `#` (relative indent kept, mixed→comment all, no-space uncomment, non-ASCII, blank line untouched, round trip, undo) |
| Random model | pass | 2 000 seeded xorshift ops (insert/backspace/delete/set_selection/undo/redo, per-op groups) vs String model: text + cursor + undo/redo availability never diverge; 20 more seeds x 2 000 ops with coalescing: same revision ⇒ same text, undo-all == initial (revision 0), redo-all == final; 2 000 malformed (overlapping/out-of-range) transactions: no panic |
| Timing (release, 100 000 lines) | info | 10 000 inserts at random lines incl. set_selection+line_to_char: 1.2 µs/insert; contiguous typing mid-doc: 0.46 µs; 10 000 separate undo groups: 0.55 µs; undo of 10 000 groups 2.0 ms |
### Contract checks
| Contract | Producer | Consumer | Result |
|---|---|---|---|
| vr-core public API (Buffer, Transaction, Change, Selection, History, edit_ops) | crates/vr-core/src | probe crate via public paths only | pass |
### Commands
`CARGO_TARGET_DIR=$TEAM_WS/qa-artifacts/T2/target $TEAM_BIN/run-heavy.sh cargo run --release --manifest-path $TEAM_WS/qa-artifacts/T2/probe/Cargo.toml`; target dir removed afterwards.
Note: probe-side mistakes found and fixed in my own test (model cursor after redo, undo count after toggle) — no library defect.

## Rule check T2
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | no crate files added/changed by QC; probe only under qa-artifacts/T2 |
| BASE-2 | pass | contract types used as in plan; additive helpers listed in notes |
| BASE-4 | pass | notes hold final test/clippy/guard output |
| BASE-5 | pass | probe run through run-heavy.sh |
| BASE-10 | pass | no workspace-wide test run by QC |
| BASE-14 | pass | run finished in foreground (<2 min) |
| BASE-15 | pass | no background tasks started |
| BASE-24 | pass | no git index changes |
| BASE-30 | pass | probe target dir deleted |
| COST-13 | pass | throwaways in qa-artifacts/T2 |

## QC T2 re-check (quick pass 1) — QC_PASS
Scope: Buffer::delete clipping, normalize ordering, random model. Probe `qa-artifacts/T2/probe/` run in release (`run_qp1_release.txt`: 90 PASS, TOTAL FAILS 0) and debug with debug_asserts active (`run_qp1_debug.txt`: 89 PASS, TOTAL FAILS 0; phase C malformed-transaction run skipped in debug because overlap now debug_asserts by design; it ran in release without panic).
Added cases (all pass, both profiles): backspace with cursor touching selection end ("abcdef" ranges 1..3 + cursor 3 -> "adef"); delete_forward cursor before selection -> "abf"; reversed-order ranges touching -> "adef"; two-selection delete -> "cf" and one undo step restores; 3 adjacent cursors backspace -> "def", delete_forward -> "aef"; cursors at 0 and 1 backspace -> "bcdef"; combining-mark multi-delete clipped without panic; duplicate cursors no panic. Random model (2 000 ops per-op groups + 20 seeds x 2 000 coalescing) still matches String model; no debug_assert fired. Target dir removed.

## QC T7 re-check (round 1)
Engine: the Rust probe `qa-artifacts/T7/probe` (mode `ac8`, rewritten for the new signatures: `trash` returns a per-path batch result, `AlreadyExists(PathBuf)`, `validate_name`, `ensure_within`). AC-9 and perf skipped as asked. Fixtures and target removed afterwards.

| Check | Result | Evidence |
|---|---|---|
| create / rename / copy / move / trash disk state | PASS | 11 base checks PASS (create over existing refused with content kept, rename file+dir, recursive copy keeps source, move_to returns new path, move into itself refused with dir intact, rename missing errors) |
| rename, move, copy onto existing file or folder refused, target unchanged | PASS | 9 checks: rename file->file, dir->dir, file->dir; move file and dir onto existing; copy file and dir onto existing (also nested dir). Each returns `AlreadyExists`, target content and source both intact, no partial merge (`tdir/f`, `dest/sdir/f` absent) |
| `a.txt` -> `A.txt` | PASS | Ok, only `A.txt` in the listing, content kept. Also `b.txt` -> existing distinct `B.txt` refused, both intact |
| batch trash of 3 with 1 missing | PASS | results in order: CtlCopy Ok, missing.bin Err("... does not exist"), renamed.rs Ok (2 ok, 1 err); docs entries 3 -> 1; temp XDG `Trash/files` = [CtlCopy, renamed.rs], `Trash/info` = the 2 `.trashinfo`; dir content preserved; user trash untouched |
| validate_name | PASS | rejects `..`, `a/b`, `""`, `.`, NUL name; accepts `ok.txt`, `.hidden`, `a b` |
| ensure_within (extra) | PASS | rejects `../x`, accepts `src/a` |

QC_PASS T7 R1

## QC T7 re-check (round 2)
Engine: the same Rust probe (`ac8` mode, release build against the round-2 vr-fs), extended with 6 extra destination-safety checks. Result: 41 PASS lines, 0 FAIL. Fixtures, target and Cargo.lock removed afterwards.

| Check | Result | Evidence |
|---|---|---|
| create, rename, move, copy, refusal onto existing target, `a.txt` -> `A.txt`, batch trash, validate_name | PASS | all round-1 results unchanged: refusals return `AlreadyExists` with target and source intact; case-only rename Ok; batch of 3 with 1 missing gives 2 ok + 1 err in order, exactly the 2 in temp XDG `Trash/files` + `info`; validate_name rejects `..`, `a/b`, `""`, `.`, NUL |
| A failing move never removes a destination file (public API) | PASS | destination `d2/victim` kept in every failing case: move of a missing source, move into own parent, move of a dir onto a same-named dest file, rename of a missing source onto it, rename of a file under itself, move into a read-only dest dir (refused as `already exists`, source kept) |
| Hard-link fallback | n-o | cannot be forced through the public API (needs a filesystem where `renameat2` NOREPLACE is unsupported); not observed, covered by developer tests only |
| `BothNamesLeft` / `LeftAt` / symlink trash message | n-o | error-path variants reached only by a failure partway through the fallback, not reproducible from outside |

QC_PASS T7 R2

### Readonly-msg replay (round 2 follow-up)
WITHDRAW T7 readonly-msg. In my R2 probe the destination name already existed (`d2/victim` created earlier in the same case), so `AlreadyExists` was the correct error. Replay on the current tree (uid 1000, ext4 reported by `stat -f` as ext2/ext3, dest dir set read-only, source `x1/...`):
- A, `move_to(x1/victim, d2)` with `d2/victim` present: `AlreadyExists(.../d2/victim)`; dest content kept.
- B, `move_to(x1/fresh, d2)` with the name absent: `Io { path: .../d2/fresh, source: PermissionDenied (os error 13) }`, matching the reviewer.
Fixtures and target removed.

## QC T7 re-check (round 3)
Engine: the same Rust probe (`ac8`, release build on the round-3 tree). Result: 41 PASS, 0 FAIL. This covers every R2 case: create, rename, move, copy, refusal onto an existing target, `a.txt` -> `A.txt`, batch trash (2 ok + 1 err, 2 in the temp XDG trash), validate_name, ensure_within, and the 6 failing-move cases (the destination file is kept each time). `find fx8 -name '*vr-tmp*'` after the run: 0 files. Fixtures, target and Cargo.lock removed.

QC_PASS T7 R3

## QC T9 — vr-search engine (QC_PASS)
Engines: Rust only (no UI). Build: working tree on base c421310 + uncommitted T4/T9 files; probe crate `qa-artifacts/T9/probe` (path deps on vr-search, vr-core), release build, `git grep` as oracle (`rg` also present, not needed). Developer's tests not rerun.

| AC | Status | Evidence |
|---|---|---|
| AC-9 | pass | Fixture git repo: root `.gitignore` (node_modules/, vendor/, *.log), `sub/.gitignore` (secret.txt, build/), nested `pkg/vendor/`, hidden `.env`/`.hid/`, a binary, an untracked file and an untracked ignored `.log`. File set of `search_project` == `git grep --untracked -I -l` (7 files, diff empty). No ignored file returned (node_modules, vendor, pkg/vendor, *.log, sub/secret.txt, sub/build); hidden files included (D15); binary skipped. `qa-artifacts/T9/ac9_*.txt` |
| AC-10 | pass | `replace_all_tx` `fn (\w+)` → `fn new_$1` on text with Vietnamese lines before matches (incl. a CRLF line): 3 changes in one Transaction; result equals expected string exactly; via `vr_core::Buffer`: one `undo()` restores the original, second `undo()` = false, `redo()` reapplies. |
| AC-11 | pass | 50 000 files in 500 dirs (git-init'd), 1 336 matches, release, warm cache: first Batch 6.1 ms / 6.0 ms, Done 30.0 ms / 26.2 ms (2 runs, 7 batches, files=50000). Cold cache not measured (no drop_caches without root); developer's cold run on 81k files: first batch 60 ms, done 2.1 s. Positions vs `git grep -n --column` (first match per line, 820 lines): path and line agree on all 820; column agrees on all 304 ASCII lines; the 516 differences are all on Vietnamese lines and all exactly +8 for git (git counts bytes; vr-search counts chars, e.g. "Tiếng Việt ở đây zebraquux": ours 18, git 26). Per contract (D34) col counts chars, so vr-search is right. 20 sampled lines (seeded shuf) all consistent with this. CRLF lines: line_text carries no `\r`. |
| AC-12 | pass | 5 files (CRLF, BOM, no final newline mode 0750, Latin-1, binary): previews for the 3 valid ones (2 hunks each), refusals for Latin-1 ("not UTF-8; …") and binary ("binary file"); plan wrote nothing (all bytes identical). Apply with crlf+nonl confirmed: both `byte_exact` (`BAR one\r\nbar\r\nBAR two\r\n`, `a BAR\nb BAR`), nonl mode still 0750, unconfirmed BOM file / Latin-1 / binary byte-identical, no temp files left. Confirmed file modified between preview and apply → `Err("file changed on disk since the preview; not written")`, content stays the new content; a fresh plan+apply then writes the BOM file byte-exact (`EF BB BF` kept). |

Known limits (not defects): owner/group not restored on apply (D37, documented). `git grep` prints only the first match per line, so per-line comparison used the first match.

## Rule check T9
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-search/src/{lib,find,project,replace}.rs for T9 per notes; QC changed no production file |
| BASE-2 | pass | public items match plan Contracts (Query, find_all, replace_all_tx, ProjectMatch, SearchMsg, search_project, plan_replace, apply_replace); additive Hunk/replacements()/&-params listed in notes; probe compiled against them |
| BASE-3 | pass | notes list red lines / mutation-red per test |
| BASE-4 | pass | notes hold trimmed test/clippy/fmt output; guard failure confined to vr-core T4 files |
| BASE-32 | pass | D15 (AC-9 probe), D31/D33/D34 (char col, line numbers vs git), D36 (empty line), D37 (stale file refused) verified |
| RF-4 | pass | streaming: first batch 6 ms, 7 batches ≤200 on 50k files, Done last |
| RF-5 | pass | one undo for replace-all (AC-10 probe) |
| BASE-14/15/17/18 | pass | all QC commands foreground and short; no kill, no sub-agent; probe fixtures removed (`du` of qa-artifacts/T9 small) |

QC_PASS T9

## QC T4 — QC_PASS
Build: commit 0e85a06 plus the working tree of crates/vr-core. Engine: Rust probe crate (`qa-artifacts/T4/probe`, path dep on vr-core, release, own target dir). No browser. Logs: `qa-artifacts/T4/{corpus,watch,big}.log`. Developer tests not rerun.
| AC | Status | Evidence |
|---|---|---|
| AC-3 | pass | 46 PASS lines in corpus.log, 0 FAIL: LF, CRLF, lone CR, mixed, BOM, no final newline, Vietnamese, empty — edit then undo gives `is_modified()` false; edit+save bytes equal expected exactly (BOM kept, line endings untouched); unmodified after save, modified after undo past save, unmodified after redo. 0o755 script keeps mode 755. Symlink (relative, and absolute into another dir) stays a symlink and the target gets the content. No temp files in `ls -a` of the corpus or target dir. Latin-1, UTF-16LE, UTF-16BE open `read_only`; `save`, `save_overwrite`, `save_as` all return Err (`NotUtf8ReadOnly` for save), file bytes unchanged, no file created by save_as. |
| AC-7 | pass | watch.log, 17 PASS: external `>>` gives Modified in 201 ms; atomic temp+rename gives Modified (twice in a row, watch survives replace); `rm` gives Removed; own `save()` and `save_overwrite()` (with `ignore_own_write`) give no event in 1.5 s; external write 100 ms after own save is reported (Modified). `check_disk()` returned ChangedOnDisk / InSync / DeletedOnDisk correctly; `save()` returns `ChangedOnDisk` with the disk file untouched; `save_overwrite()` succeeds; `reload()` returns to InSync and unmodified; buffer kept after delete; `save()` recreates a deleted file. |
| AC-18 | pass | 104,857,638 bytes (1,436,407 lines), release: open 123 ms (limit 2 s). Peak RSS 222,844 kB (about 218 MB, 2.1x file size; VmRSS after open 120,748 kB). |
### Rule check T4
| Rule | Result | Evidence |
|---|---|---|
| AC-3 / AC-7 / AC-18 | pass | rows above |
| BASE-2 (contracts) | pass | probe compiled against plan signatures; additive items already listed in notes (`watch -> Result`, `DocError::{ChangedOnDisk,NoPath}`, `LineEnding::Cr`) |
| BASE-4 (dev evidence) | pass | notes hold test, fmt and clippy output |
| BASE-5 / BASE-14 / BASE-15 | pass | release build via run-heavy.sh; all runs foreground, under 5 s; nothing left running |
| BASE-17 | pass | no database or app touched; fixtures only under qa-artifacts/T4 |
| BASE-24 | pass | no git index changes |
| COST-13 | pass | throwaway files only in qa-artifacts/T4, 100 MB file, fixtures and target dir removed afterwards |
n-a: UI rules (library task, no UI)
Notes (no defect): rename-based save breaks hard links and drops xattrs/ACLs, and a read-only directory blocks save — both already documented by the developer as known limits.

## QC T4 re-check (round 1) — QC_PASS T4 R1
Build: working tree after fix round 1 (`save() -> Result<Saved,_>`, `set_own_writes`, `Rescan`/`Error`). Engine: the same Rust probe (release), updated to the new API. Logs: `qa-artifacts/T4/r1_*.log`. Fixtures and target dir removed afterwards.
| Check | Result | Evidence |
|---|---|---|
| AC-3 corpus (LF, CRLF, CR, mixed, BOM, no final newline, Vietnamese, empty, 0o755, symlinks, Latin-1/UTF-16 read-only) | pass | 46 PASS, 0 FAIL (r1_corpus.log); byte-exact; no temp files in `ls -a` |
| Unwritable files | pass | 0o444 file in writable dir and 0o464 file (we own it, no owner write bit): `save()` and `save_overwrite()` both `Err(PermissionDenied)`; content, uid/gid, mode, inode unchanged; no temp file. (The one FAIL line for m464 in r1_unwritable.log is a probe bug: it expected 2 directory entries but only 1 file existed yet; the entries list shows only `m464.txt`.) |
| Concurrent writer, 50 saves per run, writer rewriting every ~1 ms, start offset swept | pass | Four sweeps (200/400/800/2000 us step): ok=23/37/43/47, ChangedOnDisk=27/13/7/3. Every Err left the writer's content in the file; every Ok left either our bytes or later writer bytes; 0 odd contents, 0 leftovers. Zero cases where our save replaced a writer version that landed before or during `save()` (Ok + our bytes + writer write during save: 0 of 150 Ok-with-ours cases checked; Ok after a prior writer version: 0). With an 17 MB doc, 20 ms sweep: ok=48, ChangedOnDisk=2, same invariants. Control with the writer delayed 500 ms: Ok. |
| AC-7 with `set_own_writes` | pass | r1_watch.log, 18 PASS: append gives Modified in ~200 ms; atomic replace twice gives Modified; rm gives Removed; own `save()` / `save_overwrite()` give no event in 1.5 s; external write 100 ms after our save is reported; `check_disk` states correct; `save_overwrite` ok after ChangedOnDisk |
| Churn: 2 000 files rewritten via temp+rename (+ in-place writes, remove+create) in about 0.5 s, dir and sub-dir watched | pass | events=2000, distinct paths reported=2000/2000, Rescan=0, Error=0: not silent, every file reported |
Notes: the Rescan path itself was not provoked (the kernel queue did not overflow); it is covered only by the developer's unit test.

## QC T9 re-check (round 1) — QC_PASS T9 R1
Same probe (`qa-artifacts/T9/probe`, release) against the tree after fix round 1; vr-core compiled.

| Check | Result | Evidence |
|---|---|---|
| AC-12 again | pass | Same 5-file scenario: previews for crlf/bom/nonl, refusals for Latin-1 and binary; apply with crlf+nonl confirmed gives byte-exact `BAR one\r\nbar\r\nBAR two\r\n` and `a BAR\nb BAR`, mode 0750 kept; unconfirmed bom/latin1/bin byte-identical; file changed after preview → `Err("file changed on disk since the preview; not written")` and keeps the new bytes; no stray temp files. |
| Concurrent writer | pass | 200 confirmed files of ~110 KB, victim rewritten every ~1 ms by a background thread (writes alternate between content equal to the preview and unique content). Two writer modes (in-place truncate+write, temp+rename), 60 runs each (120 total, above the 20 asked; a first 20+20 batch with a weaker writer also clean). Rule: final content must equal the writer's last write or its clean replacement. Result: 0 dropped or torn files in 120 runs; 46 runs skipped the victim with the error item "file changed on disk since the preview; not written"; 74 runs applied Ok and the writer's later write then won (final = writer's latest content, allowed). The other 199 files were clean replacements with Ok in every run; no leftover temp files. Logs `race_inplace.txt`, `race_atomic.txt`. Note: a writer that holds the old file open across the exchange is the documented remaining window and was not simulated. |
| 40 MB file | pass | 41 943 056-byte non-ignored file: Batch (small.txt) → `Error "1 file larger than 32 MB not searched"` → `Done files=1 matches=1`; `plan_replace` on it refuses with "larger than 32 MB; replace refused". |
| `foo(?-m)$` | pass | search_project = find_all = 7 on LF/CRLF/lone-CR files; also equal for `foo$`, `(?m)foo$`, `(?i)FOO(?-m)$`. |
| `fn $1_old` | pass | `fn a` → `fn a_old` via `replace_all_tx` and via project plan+apply (`fn a_old\nfn bc_old() {}\n`). |

Cleanup done: fixtures, target dir, Cargo.lock removed; only probe source and small logs remain.

QC_PASS T9 R1

## QC T4 re-check (round 2) — QC_PASS T4 R2
Build: working tree after fix round 2 (API unchanged). Engine: Rust probe (release); logs `qa-artifacts/T4/r2_*.log`; fixtures and target dir removed afterwards.
| Check | Result | Evidence |
|---|---|---|
| AC-7 own save, then external write 100 ms later | pass | own save: no event in 1 s; external write 100 ms later: Modified; `check_disk` ChangedOnDisk (r2_revert.log) |
| Revert to the saved bytes: save C1, external C2, `reload()`, file written back to exactly C1 | pass | in-place write of C1 bytes: Modified event, `check_disk()` = ChangedOnDisk, `save()` refused with ChangedOnDisk; variant temp+rename back to C2 bytes: Modified |
| Concurrent writer, 2 runs x 50 saves, two start-offset sweeps each (400 us and 2000 us steps) | pass | Ok/ChangedOnDisk: run1 44/6 and 48/2, run2 39/11 and 48/2; plus a 17 MB doc run 48/2. 0 bad contents, 0 leftovers, no case of our bytes replacing a writer version that landed during `save()` (INFO count 0 in every run) |
| One `chmod` by another thread at a time inside `save()` (17 MB doc, save about 37 ms, chmod offset swept 0 to 19.6 ms in 50 steps) | pass | 50 of 50 saves Ok, none ChangedOnDisk, file holds our bytes, no leftovers (r2_chmod_once.log). Control: one chmod 5 ms before the save: 50 of 50 Ok |
Note (not a defect against the criterion): a continuous chmod storm (one chmod every 50 us for the whole save) makes every save return ChangedOnDisk (50 of 50): the pre-rename stamp check sees ctime moving. A single chmod, however timed, does not.
