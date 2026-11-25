
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

## QC T9 re-check (round 2) — QC_PASS T9 R2
Same probe (release) after round 2.

| Check | Result | Evidence |
|---|---|---|
| AC-12 core | pass | Byte-exact `BAR one\r\nbar\r\nBAR two\r\n` and `a BAR\nb BAR`, mode 0750 kept, unconfirmed bom/latin1/bin byte-identical, Latin-1 and binary refused, changed-since-preview file skipped with "file changed on disk since the preview; not written" and keeps new bytes, no stray files. |
| Concurrent writer, 2 x 30 runs | pass | 200 files, victim rewritten every ~1 ms (in-place and temp+rename writers). 0 dropped or torn victims in 60 runs; 18 runs skipped with an error item, 42 applied Ok (writer's later write then won). Other 199 files clean in all 60 runs. Leftovers: in all 42 runs without an error, no `.vr-ide-save-*` / `.vr-ide-conflict-*` (or any non-fN.txt) file remained; the 18 error runs left none either. Logs `race2_inplace.txt`, `race2_atomic.txt`. |
| `(?x)foo # c` whole word | pass | text `foo / foobar / xfoo / a foo b`: find_all ranges [(0,3),(18,21)], search_project [(1,1),(4,3)] — only the whole-word occurrences. |

Cleanup done; probe source and race2 logs remain.

QC_PASS T9 R2

## QC T5 — QC_FAIL T5 (one racing-writer data-loss case, core save path)
Build: release `target/release/vr-ide` built from commit 1188018 + dirty tree (13 changed/untracked entries, T5 files uncommitted); mtime 12:40, after the last change to crates/vr-ide/src. Engine: real binary under a pty (own VT emulator in `qa-artifacts/T5/qc_flows.py`, 80x24 unless noted, no DISPLAY, `VR_IDE_CONFIG_DIR`/`VR_IDE_STATE_DIR` in per-fixture temp dirs; DA1 answered, kitty query answered only in the kitty flows). One script, one result line per flow.

| Flow | Result | Evidence |
|---|---|---|
| AC-1 title shows root | pass | OSC title `vr-ide — <fixture root>`, tab bar row shows root; 40x12 terminal starts and exits 0. Explorer tree is T8, not checked. |
| AC-3 type/Vietnamese/backspace/undo/redo/save | pass | typed "ASCII " + "Tiếng Việt"; `●` before save, gone after ctrl+s; disk `ASCII Tiếng Việtbase\n`. Exact-bytes run: "ab"+"Tiếng Việt"+"e◌́", backspace removes the whole grapheme, backspace removes "t" → disk `abTiếng Việ` (want identical). ctrl+y and, in kitty mode, ctrl+shift+z (`CSI 122;6u`) redo → disk `one`. |
| AC-4 copy/cut/paste, OSC 52 | pass | no DISPLAY: copy and cut each emit `ESC]52;c;aGVsbG8=` (decodes `hello`); in-app paste after copy+cut → disk ` worldhellohello\n`. |
| AC-5 user keymap | pass | `ctrl+s=file.save_as`, `ctrl+t=no.such_command` → startup notice `keymap: ctrl+t is bound to unknown command "no.such_command" (global)`, no crash; ctrl+s then does not write (T6 prompt pending). A keymap with `ctrl+s=file.save` saves (`Yabc\n`). |
| AC-7a unmodified file `echo >>` | pass | new content on screen after 0.204 s. |
| AC-7b modified buffer + external change | pass | tab `!`, status `[changed on disk]`, buffer `Mone` kept, ctrl+s leaves disk = external version. |
| AC-7c Save (overwrite) | pass | ctrl+s refused (disk `one\next\n`); only the bound `file.save_overwrite` command (ctrl+u in the test keymap) writes `Mone\n`. |
| AC-7d external delete | pass | buffer kept, tab `w.txt (deleted)`. |
| Concurrent writer, 30 runs (script) | FAIL | writer rewrites every ~1 ms (in-place and temp+rename alternating, start offset -2..+3 ms around the ctrl+s byte). 27 refused, 3 saved, 0 torn, 1 violation: run 11 (rename mode, +3 ms): save reported success and a complete writer version was replaced by our bytes, then later writer versions overwrote ours. |
| Concurrent writer, extra 150 runs (temp+rename writer, +2.0..+4.75 ms) | FAIL | 41 saved, 109 refused, 0 torn; 1 more lost-write case (run 71, +3.25 ms: writer version 1 replaced by our save, save reported success). Total 2 lost writes in 180 racing runs. strace of a saved run shows the last `statx` of the target 43 us before `rename(.vr-ide-save-*.tmp, r.txt)` — the window that `write_atomic` documents (D25, no compare-and-swap rename). |
| AC-18 100 MB | pass | 104857600 bytes; first frame 0.123 s after spawn; pgdn x50 latency (key sent → screen changed, 5 ms poll): median 12.4 ms, p95 12.9 ms, max 14.2 ms; RSS 122 MB after scrolling. |
| Recovery after forced panic | n-o | no debug hook that panics the binary; SIGABRT/SIGKILL are not panics. Covered only by unit test `panic_hook_writes_recovery` (mode 0600, project file untouched); terminal restore after a panic not run on the real binary. |

Defect 1 (blocker by the ledger severity anchor; code is vr-core `Document::save` → `write_atomic`, T4 / D25, reached through T5's ctrl+s): a writer replacing the file in the ~40 us between the last freshness `statx` and `rename(2)` is silently replaced and the app reports a clean save.
Repro: fixture `r.txt` = `base\n`, `vr-ide r.txt`, type `X`; a second process starts `rename()`-ing complete versions over `r.txt` every ~1 ms starting 3 ms after the byte `\x13` (ctrl+s) is written to the pty; about 1 in 40 saves that succeed loses a writer version (script: `qc_flows.py <bin> <dir> CONCURRENT`). Expected: save refused with the notice, or no writer version replaced. Needs a decision by the leader: either accept D25 as the documented limit, or re-open T4.

Observation (minor, not blocking): in a terminal without the kitty protocol the persistent "this terminal does not report these keys…" startup warning fills the 3 notice rows, so the ctrl+s refusal notice is not visible until Esc is pressed 3 times; the tab `!` and status `[changed on disk]` do show. With kitty keys the refusal notice shows (`w.txt changed on disk; your unsaved changes are kept`).

Cleanup: fixtures, the 100 MB file and probe scripts removed; only `qc_flows.py` (plus the developer's `pty_smoke.py`, `release_checks.sh`) remain in `qa-artifacts/T5/`. No processes left (all pty children killed by pid).

## Rule check T5
| Rule | Status | Evidence |
|---|---|---|
| BASE-23 every entry path | pass | save by ctrl+s and by command (`file.save_overwrite`, `file.save_as`); copy/cut/paste by key; external change by real watcher on the real binary. |
| BASE-26 | n-a | not a rewrite. |
| BASE-9 / ledger concurrent-writer check | fail | see Defect 1: 2 lost writes in 180 racing runs, via vr-core write_atomic (documented D25 window). |
| BASE-14 | pass | no waiting loops; builds in foreground via run-heavy (6 s). |
| BASE-15 | pass | killed only own pty children by pid. |
| BASE-17 | pass | probes wrote only to fixtures under `qa-artifacts/T5/`, removed. |
| BASE-19 | pass | each flow's result line written to this report after it ran. |
| BASE-24 | pass | no git index/history commands (`git init` only inside fixtures, deleted). |
| BASE-46 | pass | evidence on binary built 12:40 from the current tree (commit 1188018 + dirty). |
| COST-1 | pass | release build through run-heavy.sh. |
| COST-13 | pass | scripts and fixtures under `qa-artifacts/T5/`. |
| AC-1, AC-3, AC-4, AC-5, AC-7, AC-18 | pass | rows above. |

## QC T4 re-check (round 3) — QC_PASS T4 R3
Build: worktree `.worktrees/T4r3` (branch t4-round3), `crates/vr-core` and `crates/vr-search` via path dependencies, own target dirs. Engine: Rust probes (release). Logs: `qa-artifacts/T4/r3/`. Fixtures, target dirs and the 100 MB file removed afterwards.
| Check | Result | Evidence |
|---|---|---|
| AC-3 corpus (LF, CRLF, CR, mixed, BOM, no final newline, Vietnamese, empty, 0o755, symlinks, Latin-1/UTF-16 read-only) | pass | 46 PASS, 0 FAIL; byte-exact; mode and symlinks kept; `ls -a` shows no temp or conflict files. Unwritable 0o444 file: refused (pass). 0o464 file: refused with PermissionDenied (the one FAIL line is the known probe entry-count bug from round 1, entries list shows only the file) |
| Concurrent writer, 4 runs x 50 at the round-2 offsets (400 us and 2000 us steps, 8 loops, 400 saves) | pass | Ok/ChangedOnDisk at 400 us: 43/7, 42/8, 43/7, 42/8; at 2000 us: 48/2 in all four. Every Err left the writer's content; every Ok left our bytes or later writer bytes. Our-bytes-with-writer-write-during-save (replaced a writer version): 0 in all 8 loops. 0 bad contents, 0 leftover or conflict files |
| Revert (save C1, external C2, reload, file back to C1 bytes) | pass | own save no event; external write 100 ms later Modified; revert to C1: Modified, `check_disk()` ChangedOnDisk, `save()` refused; temp+rename revert: Modified |
| AC-18 100 MB open | pass | 104,857,638 bytes, 1,436,407 lines, open 116 ms and 120 ms (limit 2 s); peak RSS 223,028 kB (about 218 MB) |
| vr-search probe (T9) AC-12 core | pass | plan writes nothing; CRLF and no-final-newline files byte-exact after apply; unconfirmed BOM file, Latin-1 (refused "not UTF-8") and binary files untouched; mode 750 kept; stale file refused ("changed on disk since the preview; not written") and its bytes kept; BOM apply after re-plan exact; directory has no extra files |
| vr-search racing, 2 x 30 runs (atomic writer and in-place writer, 200 files, victim file rewritten every ~1 ms) | pass | atomic: dropped_or_torn 0, 14 refused, 16 Ok, other files all clean, 0 extra files; in-place: dropped_or_torn 0, 12 refused, 18 Ok, other files clean, 0 extra files. The "unchanged_but_apply_ok" figure (16 and 18; round 2 was 19) is the writer's own later write of content identical to the preview, which cannot be told from a lost replace by content, and the T9 probe counts it that way too; no torn or other content seen |

## QC T5 re-check (round 1) — QC_PASS T5 R1
Build: release `target/release/vr-ide` rebuilt 13:01 from commit 1188018 + dirty tree (19 changed/untracked entries; newest src edit 12:58:59). Same pty harness (`qa-artifacts/T5/qc_flows.py`, flows AC-3, AC-7, R1), concurrent-writer flow skipped as instructed (T4 escalation, re-test when it lands in vr-core).

| Check | Result | Evidence |
|---|---|---|
| AC-3 Vietnamese typing, grapheme backspace, undo/redo (ctrl+y, kitty ctrl+shift+z), save | pass | disk `ASCII Tiếng Việtbase\n`; `abTiếng Việ` exact; redo → `one`; `●` before save, gone after. |
| AC-7 a/b/c/d | pass | reload in 0.203 s; modified+external: tab `!`, buffer kept, ctrl+s refused, overwrite only via `file.save_overwrite` → `Mone\n`; delete keeps buffer. |
| Hold ctrl+w 1 s (30 presses) on modified tab | pass | tab stays open, notice `m.txt has unsaved changes: ctrl+w / ctrl+f4 again to close without saving`; file untouched. (Recovery dir empty: nothing closed.) |
| ctrl+q, Esc, wait 1 s, ctrl+q | pass | process alive, warning `1 unsaved file(s): ctrl+q again to quit without saving` shown again. |
| Bracketed paste `a\rb` and `a\r\nb` into LF file, save | pass | disk `a\nbx\n` both cases, no CR. |
| 10 MB single line, cursor at end (Ln 1, Col 10485771) | pass | first frame 0.022 s; End 11.4 ms; per key (30 arrows + 10 typed, 5 ms poll): median 10.9 ms, p95 11.7 ms, max 11.8 ms. |
| Legacy terminal (no kitty): ctrl+s refusal visible without Esc | pass | bottom row shows `w.txt changed on disk; your unsaved changes are kept  (+1 more · Esc)` immediately after ctrl+s. |
| 40x12 status bar file name | pass | ` name_of_file.txt · Ln 1, Col 1`, still visible after typing. |

Cleanup: fixtures removed, no processes left, no git index changes.

## QC T4 re-check (escalation) — QC_PASS T4 ESC
Build: worktree `.worktrees/T4r3` (D73-D76, D64 API), `vr-core` and `vr-search` by path dependency, release probes, own target dirs. Logs: `qa-artifacts/T4/esc/`. Fixtures, target dirs and the 100 MB file removed afterwards.
| Check | Result | Evidence |
|---|---|---|
| AC-3 corpus | pass | 46 PASS, 0 FAIL; byte-exact, mode and symlinks kept, no temp or conflict files. Unwritable 0o444: refused. 0o464: refused PermissionDenied (the single FAIL line is the known probe entry-count bug) |
| Concurrent writer, 4 x 50 at round-2/3 offsets (400 us, 2000 us steps) | pass | 400 us: Ok/ChangedOnDisk 42/8, 43/7, 43/7, 43/7; 2000 us: 48/2 in all four. Every Err left the writer's content; every Ok left our bytes or later writer bytes; our-bytes-replacing-a-writer-version 0 of 8 loops; 0 bad contents |
| Revert case | pass | own save no event; external write 100 ms later Modified; revert to C1 bytes: Modified, `check_disk()` ChangedOnDisk, `save()` refused; atomic revert Modified |
| AC-18 | pass | 104,857,638 bytes: open 150 ms and 116 ms; peak RSS 222,976 kB |
| vr-search AC-12 core | pass | plan writes nothing; CRLF / no-final-newline byte-exact; unconfirmed BOM, Latin-1, binary untouched; mode 750 kept; stale file refused; no extra files |
| vr-search racing 2 x 30 | pass | atomic writer: dropped_or_torn 0, 19 Ok, 11 refused; in-place writer: dropped_or_torn 0, 15 Ok, 15 refused; other files clean, 0 extra files. ("unchanged_but_apply_ok" 19 and 15 = writer's own later write of preview-identical content, as in earlier rounds) |
| D64 `disk_version()` + `save_overwrite_seen` | pass | no external change: Ok, our bytes on disk. External in-place write after `seen`: ChangedOnDisk, newer external content on disk. Same-size external change: ChangedOnDisk. Atomic external replace: ChangedOnDisk, external content kept. Prompt flow (save refused, `seen` taken after the change): Ok. Racing loop, 50 saves with a writer at three offsets: SJ=100 ok 18 / ChangedOnDisk 32; SJ=300 38/12; SJ=1000 46/4; 0 bad: every Err left writer content, every Ok left our bytes or later writer bytes, no Ok after a writer version existed before the call |
| `.vr-ide-conflict-*` files | pass | 0 appeared in any run (corpus, 8 concurrent loops, D64 loops, both vr-search race runs) |

## QC FR-2 — QC_PASS FR-2
Build: worktree `.worktrees/FR2` (branch fr2-save-as-seen), `crates/vr-core` by path dependency, release probe with its own target dir. Logs: `qa-artifacts/T4/fr2/`. Fixtures and target dir removed afterwards.
| Check | Result | Evidence |
|---|---|---|
| (a) seen existing B, external write, `save_as_seen(B, seen)` | pass | Err(ChangedOnDisk); B keeps "b external"; document path unchanged |
| (b) seen absent C, C created externally, `save_as_seen(C, seen)` | pass | Err(ChangedOnDisk); C untouched ("c external") |
| Own path | pass | external change then `save_as(same path)`: ChangedOnDisk, external content kept; `save_as_seen(own, stale seen)`: ChangedOnDisk; own path spelled `dir/./name`: ChangedOnDisk |
| No change | pass | existing target Ok, absent target Ok, own path `save_as` Ok, own path `save_as_seen` Ok; each wrote exactly our bytes and set the document path (own path unchanged) |
| Concurrent writer on `save_as_seen`, 2 x 50 for an existing other file and 2 x 50 for an absent name (in-place writer step 300 us; temp+rename writer step 1000 us) | pass | existing: Ok/ChangedOnDisk 39/11 and 47/3; absent: 39/11 and 46/4. Ok after a writer version existed before the call: 0 of 200 saves. Ok with our bytes although a write landed during the call: 0. Every Err left the writer's content and the document path unchanged; every Ok left our bytes or later writer bytes; 0 bad contents |
| Temp and conflict files | pass | none after the fixed cases and after all 200 racing saves (0 `.vr-ide-conflict-*` files, so none needed to be named in an error) |
| AC-3 corpus regression | pass | 46 PASS, 0 FAIL |

## QC T6 — QC_PASS
Build: release binary from commit 663b515 plus a dirty working tree (T6 changes uncommitted; FR-1 may touch vr-core testutil.rs, test-only). Script: `qa-artifacts/T6/qc_flows.py` (reuses the T5 pty harness, one result line per flow), run under a pty at 80x24 and 40x12 with `VR_IDE_CONFIG_DIR` and `VR_IDE_STATE_DIR` in fixture dirs; the work dirs were removed afterwards.
Engine: pty with a VT emulator; no browser.

| AC / check | Status | Evidence |
|---|---|---|
| AC-6 F1 opens; "sav" lists Save with ctrl+s; Enter saves; Esc closes; Down/Up; click on a row; kitty ctrl+shift+p | pass | FLOW AC-6 (8 steps true). Down,Down,Enter opens Save As; Down,Down,Up,Enter does not |
| AC-6 go to line | pass | "5:3" -> status Ln 5, Col 3. "999" -> Ln 11, the last line of a 10-line file with a trailing newline. "abc" -> the dialog stays open with the message "Line (1-11), optionally :column" and the cursor does not move |
| AC-7 modified buffer, external write | pass | the prompt shows Reload / Keep mine. Reload shows the disk text and ctrl+z brings back "Xbase". Keep mine writes only after the save prompt: disk = our bytes |
| AC-7 newer write after the prompt opened | pass | re-prompt "changed again on disk; nothing was replaced"; disk keeps the newer external content |
| AC-7 deleted on disk | pass | Save (recreate) recreates the file with our bytes. Close writes a recovery copy with mode 0600 and the file is not recreated |
| AC-7 quit with 2 modified buffers | pass | Save all / Discard / Cancel. Cancel by click and by Esc leaves the app running. Discard exits 0, stderr names both recovery files (0600, correct bytes), disk untouched. Save all exits 0 with both files written. Held Enter for 1 s (kitty repeat events) chooses nothing |
| Save As onto an existing file | pass | Replace dialog; Cancel by click, by Enter (default) and by Esc leaves other.txt byte-identical. Replace writes the file |
| Concurrent writer (ledger rule) | pass | 120 Keep-mine choices with a writer rewriting every 1 ms. Writer started 50 ms before the click: 30/30 refused with re-prompt. Writer racing the click (0-4 ms jitter): 30/30 refused with re-prompt. Writer started after the click (60 runs): 58 refused with re-prompt, 1 Ok applied before the writer wrote anything (no writer version lost), 1 refused with no re-prompt visible, disk not overwritten. Violations: 0/120 |
| 40x12 snapshots | pass | the palette and the quit dialog render without panic and all three buttons are visible (`qa-artifacts/T6/tiny_snapshots.txt`) |

Observations, not defects:
- The default button of the quit dialog is Save all, so one real Enter press saves both files and exits (control flow `AC-7-quit-enter-control`).
- Held-Enter protection depends on the terminal reporting auto-repeat events (kitty protocol). A legacy terminal sends plain Enter presses, which the app cannot tell from a real press.
- The shell command that checks the race window outside the app (between the version re-check and the rename) cannot be hit from outside. The developer's notes say this is the interim adapter, and the swap to vr-core `save_overwrite_seen` is still pending. The racing-writer test belongs with that swap (T12). 0 violations in 120 runs does not prove the window is closed.

## Rule check T6
| Rule | Result | Evidence |
|---|---|---|
| BASE-9 / ledger concurrent-writer check | pass | the 3 race modes above, 0 violations in 120 |
| BASE-23 entry paths | pass | palette by F1, by kitty ctrl+shift+p and by mouse; dialogs by keyboard and by mouse |
| BASE-32 decisions honoured (D57, D64, D65) | pass | no press-twice flow; the seen-version re-check re-prompts |
| BASE-35 / USER-1 palette keys come from the keymap | pass | the palette shows ctrl+s, ctrl+shift+s, ctrl+g from keymap.toml |
| BASE-46 commit named | pass | 663b515, dirty tree |
| BASE-3, BASE-4, BASE-25 (developer evidence) | pass | notes `04_dev_T6.md` hold red lines, test output and guard output |
| BASE-17 (probe cleanup) | pass | work dirs removed, no vr-ide process left (0) |

## QC FR-2 re-check (round 1) — QC_PASS FR-2 R1
Build: worktree `.worktrees/FR2` (D89), `crates/vr-core` by path dependency, release probe, own target dir. Logs: `qa-artifacts/T4/fr2r1/`. Fixtures (FIFO, directory, race dirs) and target dir removed afterwards.
| Check | Result | Evidence |
|---|---|---|
| Plain `save_as` (unprompted) to another existing file, writer racing, 2 x 50 | pass | in-place writer step 300 us: Ok/ChangedOnDisk 40/10; temp+rename writer step 1000 us: 47/3. Ok saves that replaced a writer version written during the call: 0 of 100; Ok saves that replaced a version existing before the call: 0 (the plain call refuses those too); every Err left the writer's content and kept the document path; every Ok left our bytes or later writer bytes; no extra files |
| `DiskVersion::of_path` on `/dev/zero`, a FIFO, a directory | pass | each returns Err "not a regular file" in about 0.1 ms (limit 1 s) |
| Save As to a FIFO / directory / `/dev/zero` (`save_as` and `save_as_seen`) | pass | every call returns an error within 0.2 ms, none blocks, document path kept |
| FR-2 regression: `save_as_seen` races (existing and absent, 2 x 50 each) and the fixed cases | pass | Ok/ChangedOnDisk 41/9, 47/3, 41/9, 47/3; 0 Ok after a writer version existed, 0 conflict files |
| AC-3 corpus smoke | pass | 46 PASS, 0 FAIL |

## QC T6 re-check (round 1) — QC_PASS T6 R1
Build: release binary rebuilt from commit e4c7cf2 plus a dirty tree (31 changed paths). Script: `qa-artifacts/T6/qc_flows.py` (all T6 flows plus the R1 flows, one result line per flow). All earlier T6 flows were re-run and still pass (AC-6, goto, reload, keepmine, newer-write, deleted, quit incl. held Enter, Save As, 40x12, Keep-mine race 120 runs).

| Check | Result | Evidence |
|---|---|---|
| Own-path Save As after an external change | pass | prefilled name + Enter gives the Keep mine / Reload prompt; disk keeps the external content |
| Replace on B changed externally | pass | "changed on disk; nothing was replaced", Replace File re-prompt, B keeps the external content |
| Save As to an absent name while a writer creates it (30 runs) | pass | 30/30 writer created the file first; writer content kept; Replace File asked. The sub-millisecond window where the app wins the race first was not reached (0 runs), so only the writer-first order is proven |
| Reload with unsaved edits | pass | exactly one recovery copy, mode 0600, content = our edit; buffer shows the disk text |
| Legacy terminal, held Enter 1 s while an external change opens the prompt | pass | the prompt appears 0.25 s after the write and is still open after 1 s; nothing chosen, disk keeps the external content |
| Deliberate single Enter after the prompt is visible (legacy) | observed | Enter at 0.1, 0.3, 0.45 s is ignored and a second press 0.8 s later acts; at 0.6 s and 0.9 s the first press acts. Consistent with the 500 ms settle |
| Deleted-on-disk prompt default | pass | buttons Save (recreate) / Close / Keep; Enter 1 s after the prompt closes it, file not recreated, tab still modified |
| Save As onto a file open in another tab | pass | unmodified other tab: Replace, then one tab (b.txt) with our bytes. Modified other tab: refused ("is open with unsaved changes"), b.txt unchanged |
| Save As to /dev/null | pass | error "/dev/null: not a regular file"; /dev/null is still a character device with size 0; a.txt unchanged, tab still modified |
| Concurrent writer on Replace, 30 + 30 runs | pass | writer racing the click: refused+re-prompt 30/30. Writer started after the click: refused+re-prompt 29, replaced before any writer write 1. Violations 0/60 |

Observation: because of the 500 ms / 700 ms Enter rules, a scripted or fast-typed Enter inside a prompt (open, type, Enter within 0.5 s of opening) is ignored, so the flows wait 0.8 s before Enter. A real typist is unlikely to hit this, but the script had to account for it.

Cleanup: work dirs removed, no vr-ide process left.

## QC T6 re-check (round 2) — QC_PASS T6 R2
Build: release binary rebuilt from commit e4c7cf2 plus a dirty tree (31 changed paths). Script: `qa-artifacts/T6/qc_flows.py`. The race flows now count a refusal as correct only when a prompt stays open AND the reason ("nothing was replaced" / "read again") is read inside the dialog box.

| Check | Result | Evidence |
|---|---|---|
| Replace race, 30 + 30 runs (writer racing the click / started after it) | pass | 60/60 refused, prompt open with the reason in the box; 0 violations |
| Keep-mine race, 30 + 30 + 60 runs (writer started 50 ms before / racing / after the click) | pass | 118 refused with prompt and reason; 2 Ok applied before any writer write (no writer version lost); 0 violations |
| Own-path Save As after an external change | pass | Keep mine / Reload prompt, external content stays on disk |
| Keep mine after a newer write | pass | the box shows "changed again on disk; nothing was replaced" and the external content stays on disk |
| Replace while a writer rewrites the target every ~1 ms for 3 s | pass | 2629 writer versions; 5 Replace clicks, 5 refused with the prompt open and "changed on disk; nothing was replaced" in the box; our bytes were never written; final disk content is the writer's last version |
| New-name Save As with a writer creating it (30 runs) | pass | 30/30 writer first, its content kept, Replace asked |

Cleanup: work dirs removed, no vr-ide process left.

## QC T8
Build: HEAD 907cdf2 + dirty tree (T8 uncommitted), release binary rebuilt via run-heavy.sh (`cargo build --release -p vr-ide`, Finished) before the run. Engine: real binary under a pty (script `qa-artifacts/T8/qc_flows.py`, harness T5/T6), 80x24 unless noted; temp VR_IDE_CONFIG_DIR/STATE_DIR/XDG_DATA_HOME under /tmp, removed afterwards. Not browser/Flutter.
| AC / item | Status | Evidence |
|---|---|---|
| AC-1 tree, folders first | pass | `FLOW AC-1+AC-9: PASS` tree=['▾ ac19','  ▸ src','  ▸ zdir','    .gitignore','    a.txt','    b.txt'] |
| AC-9 ignored hidden, toggle shows flagged | pass | node_modules/vendor/z.log hidden; toggle shows them in dim grey (SGR 38;5;8), toggle back hides |
| AC-8 create file/folder | pass | ctrl+n -> n.txt on disk and in a tab; New Folder via palette -> dir on disk |
| AC-8 rename open modified file | pass | tab renamed a2.txt, still modified, ctrl+s writes Xaye to new path, old path absent |
| AC-8 rename folder with open file | pass | tab retargeted, modified kept, ctrl+s saves under srcx/ |
| AC-8 copy/paste files | pass | "b copy.txt", "b copy 2.txt"; existing "a copy.txt" untouched (new name used) |
| AC-8 copy/paste folder in same parent | FAIL | see defect |
| AC-8 move | pass | b.txt -> zdir/b.txt; second move onto an existing name refused ("already exists; nothing was replaced") |
| AC-8 delete confirm | pass | dialog first; Enter at 0.15 s ignored; Enter later = Cancel, nothing trashed; confirm -> XDG Trash/files/a.txt + info/a.txt.trashinfo ([Trash Info], Path, DeletionDate) |
| AC-8 delete unsaved | pass | message names unsaved changes; recovery copy with content in state/recovery; then trashed |
| Safety names | pass | `../x`, `a/b`, `..` refused: dialog stays open with `invalid name ...`; nothing created inside/outside root |
| Safety root row | pass | f2 and Delete on root: no prompt/dialog, message "the project folder itself cannot be renamed, moved or deleted from the explorer" |
| Safety folder + inner file | pass | one trash entry (`src`, `src.trashinfo`), no error text |
| Leftovers | pass | notice names `.a.txt.vr-ide-conflict-1-1` and `.vr-ide-save-1-s1.tmp`; both files remain |
| External changes | pass | root create visible in 0.25 s, create inside expanded folder visible at once, outside delete disappears <1 s; expanded kept, selection kept (rename prompt names b.txt) |
| Go-to-file | pass | ctrl+p, "smrs" lists m.rs (src), Enter opens it, content shown, palette closed |
| Big tree | pass | 55 019 files (50 000 under gitignored node_modules): first frame 13 ms, keys respond 70 ms (x2 runs), no inotify warning (max_user_watches 65536), 563 watches (ignored folder not watched), RSS 7.6 MB |
| Concurrent writer vs rename, 20 runs | pass | writer rewrites every 1 ms (10 replace, 10 in-place), f2 rename: ok 20, violation 0, torn 0; renamed file holds a complete writer version, writer's last version present under old or new name, no stray files. Not run with the file open in a tab (external-change dialog would take the keys): tab-open rename is covered by AC-8 rename flows |
| 40x12 | pass | explorer + editor render, f6/ctrl+p/f2/typing without panic, process alive |

### Defect
QC_FAIL T8 — copying a folder and pasting it next to itself fails instead of making "name copy"
Repro: fixture with folder `src` and file `a.txt`; focus explorer (f6), cursor on `src`, ctrl+c, cursor on `a.txt` (target = project root), ctrl+v.
Expected: `src copy` created (files do get `b copy.txt`, `b copy 2.txt`).
Actual: error "not pasted: cannot move or copy src into itself (src)"; nothing copied.
Evidence: crates/vr-ide/src/commands/explorer.rs `copy_into` (n=0 tries `dest/src` == source); `vr_fs::ops::copy` calls `refuse_into_itself` (crates/vr-fs/src/ops.rs:353-370) before the AlreadyExists check, so IntoItself is returned and the `continue` on AlreadyExists never fires. No data loss (nothing overwritten).

## Rule check T8
| Rule | Result | Evidence |
|---|---|---|
| BASE-9 / write paths | pass | rename racing writer 20/20 no loss; delete recovery copy; move/copy never overwrite |
| BASE-21 | pass | no secrets in output |
| BASE-23 | pass | explorer by keys, palette; go-to-file by key |
| BASE-24 | pass | no git index change |
| BASE-46 | pass | runtime checks on 907cdf2 + dirty tree release build |
| BASE-15/17 | pass | no processes left (sessions killed/quit), temp dirs removed; no data committed to real stores |
| UI-fit 40x12 | pass | Tiny-40x12 flow |
| D9 delete confirm, Cancel default | pass | Delete flow |
| D76 leftovers kept, noticed | pass | Leftovers flow |
| Folder paste in place (copy naming) | fail | defect above |

## QC T8 re-check (round 1)
Build: HEAD 467e2c5 + dirty tree (26 changed/untracked paths), release binary rebuilt via run-heavy.sh before the run. Scripts: `qa-artifacts/T8/fix1_flows.py` (developer-fixed folder-copy check) via `qa-artifacts/T8/r1_flows.py` (adds the R1-* flows). Temp dirs removed.
| Item | Status | Evidence |
|---|---|---|
| All T8 flows (AC-1+AC-9, create, rename file/folder, copy-paste, move, delete, delete-unsaved, safety, nested trash, leftovers, external, go-to, 40x12) | pass | every `FLOW` line PASS |
| Folder paste in own parent | pass | R1-folder-paste: dirs ['src','src copy','src copy 2'], each holds m.rs, original intact |
| Wheel scroll 200 entries, then click | pass | wheel down: first row f000 -> f035; wheel up -> f026; click on screen row 6 (text f032.txt) opened f032.txt |
| ctrl+x then ctrl+v moves | pass | b.txt -> zdir/b.txt, gone from root; a cut onto an existing name did not overwrite |
| Big tree watches | pass | 55 019 files: 1 watch at start; expand src, mod1, mod0 = 4; collapse mod0 = 3; first frame 10 ms, keys 61 ms, RSS 7.1-7.4 MB, no inotify warning |
| Symlinked folder outside project | pass | rename answered `g.txt` and delete confirmed: both refused ("... is in a linked folder outside the project (...); rename, move and delete are refused there"), outside file intact, nothing trashed; new file created in the outside folder. The prompts open first and the refusal comes on answering. The scripted rename check in r1_flows.py reports False because the message wraps across the dialog border; checked by hand with a screen dump (outside dir stayed ['f.txt']) |
| Ignored rows marker | pass | '▸ node_modules ·', '▸ vendor ·', 'z.log ·'; a.txt has none |
| Concurrent writer vs rename, 10 runs | pass | ok 10, violation 0, torn 0 |
Note: the harness also ran inherited T6 flows; `R1-replace-race-late` (Save As/Replace, T6, not the explorer) reported 28/30 with 1 "no prompt" and 1 "replaced before any writer write" - outside T8, not re-investigated.

## QC T6 replay (late replace) — WITHDRAW
Build: release binary built in the detached worktree `.worktrees/qc-T6` (commit b787ff1, T6 and T8). Script: `qa-artifacts/T6/qc_flows.py`, flows `R3-replace-late-60` and `R3-replace-single`.

- `R3-replace-late-60`: 60 runs, 59 refused with the prompt and the reason in the box, 1 flagged "no prompt" (run 50). Screen of that run: no dialog, the tab bar shows only `b.txt` (the `a.txt` tab became `b.txt`), no modified marker, buffer shows the writer's content `w50-637`, disk holds the writer's last version. A refused Replace leaves the `a.txt` tab modified with the prompt open, so this was a Replace that applied (Save As done), after which the late writer overwrote it. The harness counted it as "no prompt" because it only looks for a dialog.
- `R3-replace-single`: 100 runs with exactly one writer version at a random 0-4 ms after the Replace click, so the outcome is classifiable from the final file. 92 refused with the prompt and "changed on disk; nothing was replaced" in the box; 8 applied first with no dialog (doc became `b.txt`, final disk = the writer's single version, written after the app). Our bytes over a writer version: 0. Refused without a prompt or without a reason: 0.

Cause: harness classification (a successful Replace that a later writer write overwrote looks like "no prompt"). The T8 QC report of 28/30 is the same pair of cases (one applied-first, one "replaced before any writer write"). D98 (a refused Replace always reopens with the reason) holds: 0 refusals without prompt in 160 runs.

## QC T10 — QC_PASS
Build: `cargo build --release -p vr-ide` rebuilt on HEAD b787ff1 with a dirty tree (T10 changes uncommitted; the release binary found was older than the sources). Real release binary under a pty, one flow script `qa-artifacts/T10/flows.py`, results in `qa-artifacts/T10/flows_result.txt` (10 flow lines, all PASS). Engine: pty + vt.py screen model (incl. background attr).

| AC | Status | Evidence |
|---|---|---|
| AC-10 find bar | pass | ac10_find_bar: "1/5" count, 5 match spans highlighted; Enter 2/5; f3 x5 3/5 4/5 5/5 1/5 2/5 (wraps); shift+f3 1/5 5/5 4/5 (wraps); alt+c 1/3, alt+w 1/4, back 1/5, alt+r `fn (\w+)` 1/3 |
| AC-10 replace | pass | ac10_replace_ascii / _vietnamese: replace-one, one ctrl+z restores; replace-all "Replaced 3 occurrences"; saved bytes == python re.sub; one ctrl+z restores original bytes (Vietnamese prefix before matches included) |
| AC-11 project search | pass | ac11_project_search: 5023-file fixture (git repo), first results 0.054 s after Enter, 101 results in 101 files grouped per file, gitignored dir and *.log never listed across all pages; Enter opens pkg28/f2807.txt Ln 12 Col 19, match selected |
| AC-11 cancel / stale | pass | ac11_enter_position_and_cancel: 5 runs of new query sent 0-16 ms after the old one: only fresh results, no stale lines |
| AC-11 large file | pass | ac11_large_file_warning: 40 MB file -> "1 file larger than 32 MB not searched" |
| AC-12 | pass | ac12_preview_checked_files: preview per file with [x]; Space unchecks f2; Enter -> confirm dialog -> only f1 (CRLF kept, 3 CRLF) and f3 changed byte-exact; f2 identical; open tab t.txt with unsaved edit changed in buffer, disk untouched, ctrl+z undoes it. ac12_changed_on_disk_skipped: file changed after preview skipped ("skipped: file changed on disk"), others replaced |
| Concurrent writer | pass | ac12_concurrent_writer: 20 runs (10 in-place, 10 temp+rename writer, ~1 ms period, started within +-2 ms of the confirming Enter), 20596 writer writes: final content == writer's last version in 20/20, lost = 0; other checked file replaced 20/20 |
| 40x12 | pass | small_40x12: find bar, replace row, search panel, preview rendered, no panic, exit 0 |

Notes (not defects): an Enter within ~0.7 s of a confirm dialog opening is ignored by design (enter settle); the flow script waits for it. Replace-one shows no notice (only replace-all does). The "terminal does not report these keys" notice appears because the pty has no kitty support. Project search reads disk, not unsaved tab buffers (dev's own note). At 40x12 the notice banner takes 3 rows.

## Rule check T10
| Rule | Result | Evidence |
|---|---|---|
| BASE-2 contracts | pass | vr-search find/search/plan/apply used as in plan; behaviour confirmed in flows |
| BASE-3/4 tests + evidence | pass | notes list red lines and 132 passed run |
| BASE-15/17 | pass | all sessions quit; 0 vr-ide processes left; scratch fixtures removed; no data committed to real stores |
| BASE-23 entry paths | pass | find via ctrl+f and ctrl+h, project via ctrl+shift+f / ctrl+shift+h |
| BASE-24 | pass | no git index changes |
| BASE-46 | pass | runtime checks on b787ff1 + dirty tree release build |
| Write path race (T7/T9 class) | pass | 20 racing-writer runs above, lost = 0 |
| UI-fit 40x12 | pass | small_40x12 |

## QC T10 re-check (round 1) — QC_PASS T10 R1
Build: worktree `.worktrees/T10r1` (branch t10-round1, HEAD 228c3d2 plus uncommitted edits), `cargo build --release -p vr-ide` in its own target/. Same pty flow script `qa-artifacts/T10/flows.py`; results `qa-artifacts/T10/flows_result_r1.txt` (14 flow lines, all PASS; concurrent writer run with 10 runs).

| Check | Status | Evidence |
|---|---|---|
| All round-0 flows (AC-10/11/12, 40x12) re-run | pass | same results as before on the new binary (first results 0.053 s on 5023 files; 40 MB file warning; CRLF/byte-exact replace; skipped-on-disk-change) |
| Stale preview | pass | preview foo->bar, "Z" typed in the replace field, Down, Enter: no dialog, all files byte-identical. No message text was seen on screen in that run |
| 2000-file preview/replace progress + Esc | pass | "Preparing preview 1984/2000", "Replacing 32/2000" shown; Esc during the replace: 64 files replaced, 1936 untouched, 0 partial/corrupt; panel reports "1936 files not replaced: ... (cancelled; not replaced)" (shown after the first notice, in the status line; per-file reasons are in the list) |
| 100 MB file find | pass | 105,000,000 B file opened in 0.13 s; ctrl+f "e" shows "1/20000+" after 0.18 s; keystroke-to-redraw latency 11-12 ms for typing and backspace in the bar, 12 ms in the editor after closing |
| 36x12 click a result, then type | pass | click on a result row opens b.rs; "Q" replaced the selected match in the editor ("// Q", saved to disk); panel query unchanged |
| 40x12 palette | pass | palette (F1, "sa") drawn above the status row; status row `a.rs · Ln 1, Col 1 · LF · UTF-8` intact |
| Concurrent writer during confirm, 10 runs | pass | 5 in-place + 5 temp+rename writers, ~1 ms period, 10281 writes: final == writer's last version in 10/10, lost = 0, other checked file replaced 10/10 |

Cleanup: scratch fixtures removed, 0 vr-ide processes left, no git index changes.

## QC T10 re-check (round 2) — QC_PASS T10 R2
Build: `.worktrees/T10r1` (HEAD 228c3d2 + uncommitted edits), `cargo build --release -p vr-ide` rebuilt. Same pty script `qa-artifacts/T10/flows.py`; full run in `qa-artifacts/T10/flows_result_r2.txt` (20 flows; the one FAIL line there, r1_click_then_type_36x12, was my script waiting only 0.6 s for the save: the key had reached the editor; with a 1.5 s wait it passed 6/6 and the script was fixed).

| Check | Status | Evidence |
|---|---|---|
| ctrl+q during "Replacing n/N" (2000 files) | pass | app stays alive, notice "Quit: a replace is running; Esc in the search panel cancels it"; replace continued to 2000/2000 files, 0 partial, no tmp files; ctrl+q quits once it is done. Variant: Esc after the refused ctrl+q cancels (64 replaced / 1936 untouched, 0 partial), then ctrl+q quits |
| Esc during replace, then ctrl+q | pass | 3 runs: quit refused until the worker has stopped, then the app exits; each file fully replaced or untouched (32 / 1968), 0 `.vr-ide-save-*.tmp`; notice names 5 files + "and 1963 more (see the panel)" (checked at 220 columns) |
| alt+c during replace | pass | notice "Replace in Project: the replace is running; Esc cancels it"; the Aa toggle attribute unchanged; replace finished 2000/2000 |
| Close the tab of a target file during the replace | pass | the buffer (already replaced in memory) is dirty, so ctrl+w asks Save/Discard/Cancel; Discard: tab closed, recovery folder holds the buffer copy with the replaced text, file on disk unchanged, other 2000 files all replaced, app alive, no tmp files |

Observations (not defects): the exit-time summary (stderr after the terminal is restored) is only printed when a job is still running at shutdown; with the normal ctrl+q path the warning is the in-app notice. Notices queue: after a refused ctrl+q the "N files not replaced" warning only shows after Esc dismisses the earlier notices (about 4 presses), and at 80x24 its 3-row notice area cuts the line before "and N more" (the full text is visible at 220 columns). Holding ctrl+q repeatedly (30 ms retries) while a replace runs slowed that replace to ~10 s for 2000 files but lost nothing. Cleanup: scratch fixtures removed, 0 vr-ide processes left, no git index changes.

## QC T10 re-check (round 3) — QC_PASS T10 R3
Build: worktree `.worktrees/T10r3` (HEAD 9cb9b00 + uncommitted edits), `cargo build --release -p vr-ide`, own target/. Same pty script `qa-artifacts/T10/flows.py`; results `flows_result_r3.txt` (full run, cut short by the 10-minute limit at r2_esc_then_ctrl_q) and `flows_result_r3_new.txt` (round-3 flows).
Environment note: the first full run overlapped with heavy disk writeback (my 2 GB stuck-replace fixture plus other load on the machine, io pressure avg10 55-65). With that load several flows (ac10 replace ascii/Vietnamese save+undo timing, r1_progress, r2_quit_*, r2_esc_then_ctrl_q) failed on timing: fsyncs took seconds, a 2000-file replace needed >60 s, no data was lost and no file was corrupt. After the pressure dropped (avg10 < 8) every one of them passed on re-run (fsync+rename of a file 3 ms, 300-file replace 1.85 s). Not an app defect; reported because the stuck-replace flow depends on it.

| Check | Status | Evidence |
|---|---|---|
| FIFO in place of a result file, preview + replace | pass | b.txt replaced by a FIFO (also held open by a reader/writer that never writes) after the search: preview ready in 0.05 s, row `[!] b.txt` with "not a regular file"; confirm "Replace 3 matches in 2 files"; a.txt and c.txt replaced byte-exact; the FIFO is still a FIFO; app responsive; ctrl+q exits at once |
| FIFOs (3, in a 2000-file result set) during the preview, then ctrl+q | pass | 3 runs, ctrl+q 10-70 ms after the preview started: exit in 0.01-0.03 s, alternate screen / mouse / paste modes restored (output ends with `?1049l ?1000l ?2004l`), FIFOs intact, other files untouched |
| Stuck replace, Esc, wait > 5 s, ctrl+q | pass | arranged with 64 files of 31 MB (two 32-file batches) on a loaded disk: after Esc "Cancelling… 0/64" stayed for 6 s; ctrl+q at 1 s refused (no dialog); at 6.5 s a confirm "The cancelled replace has not stopped after 5 s; it is probably blocked on big041.txt (a hung disk or network mount). Quit anyway leaves that file's state unknown; files not reached are not replaced." with [Quit anyway] [Cancel]; Quit anyway: "stopping the project replace (waiting up to 5 s)…" at once, process gone after 5.26 s, terminal restored, exit message "…was still writing big043.txt when vr-ide quit (blocked for more than 5 s); check that file. File state unknown: big043.txt, big050.txt, big019.txt, big044.txt, big028.txt and 57 more". On an idle disk the cancelled worker stops within 5 s, so the flow reports n-o (no stuck state can be produced without a hung mount) |
| Round-2 quit-during-replace and Esc flows | pass | r2_quit_during_replace_refused (alive, notice, 2000/2000 replaced, quit after done), r2_quit_during_replace_then_esc (32/1968, no partial), r2_esc_then_ctrl_q (3 runs, 5 files + "and 1963 more (see the panel)", no tmp files), r2_alt_c_refused, r2_close_tab (Discard keeps a recovery copy) |
| Earlier rounds' flows (AC-10/11/12, race 10 runs, 40x12, r1) | pass | all pass on the round-3 binary (race: 10 of 10 writer-final, lost 0) |

Cleanup: scratch fixtures removed (including the 2 GB set), 0 vr-ide processes left, no git index changes.

## Sign-off (machine rainy)
Commit tested: HEAD bedbf2a (master) with the leader's uncommitted workspace notes only; no product files modified. Rust 1.100.0, Linux 6.8 ext4 (CONFIG_HZ=1000). Artifacts: `qa-artifacts/signoff/` (guard.txt, suite*.txt, ignored.txt, race20.txt, bench*.txt, h.py harness + fl*.py walk scripts, proj fixture). Engine: release binary under a python pty with a small VT screen model (no browser/Flutter).

Verdict: SIGNOFF_FAIL — one deterministic unit-test failure (below), classified as a timing-dependent test, not a product data-loss defect. Everything else passes.

### Guard
`cargo fmt --all --check` OK; `cargo clippy --workspace --all-targets -- -D warnings` exit 0 (guard.txt).

### Tests (debug, once; cargo stops at the first failing crate, so the rest ran per crate with --no-fail-fast, no whole-suite rerun)
| Crate | Passed | Failed | Ignored |
|---|---|---|---|
| vr-config (unit 3 + tests/config 16) | 19 | 0 | 0 |
| vr-core (lib, includes testutil racing tests) | 141 | 1 | 1 |
| vr-fs (lib 43 + racing_writer 2) | 45 | 0 | 1 |
| vr-search (lib 37 + racing_writer 2) | 39 | 0 | 1 |
| vr-ide (lib 156, bin 0) incl. app_tests::external_modify_reloads_within_one_second_with_real_watcher (passed, no rerun needed) | 156 | 0 | 3 |
Doc tests: 0 in every crate. Total: 400 passed, 1 failed, 6 ignored.
Ignored, run release once: vr-core `open_100mb_under_2s` ok (0.42 s); vr-ide `long_line_keystroke_to_frame_fast` ok (10 MB line, worst median 224 us), `render_large_file_fast` ok (100k lines 0.99 ms, 100 MB 1.06 ms median frame), `find_in_100mb_file_stays_responsive` ok (worst key+draw 0.96 ms, capped scan 8.4 ms, full scan 196 ms). Not run: vr-search `project_search_first_batch_latency` (needs VR_SEARCH_BENCH_ROOT, manual), vr-fs `ops` helper (started by trash tests), AC-11 covered by earlier T9/T10 QC numbers (50k files: first batch 6 ms).

### Defect (1)
SIGNOFF_FAIL — `vr_core document::tests::disk_version_of_a_file_that_keeps_changing_is_an_error` fails deterministically on this machine (1 in suite + 3 of 3 reruns alone = real, not flaky).
Repro: `cargo test -p vr-core --lib disk_version_of_a_file_that_keeps_changing`
Expected: `doc.disk_version()` returns `Err(DocError::Io)` after 5 tries because the ReadingDiskVersion hook rewrites the file (same length, "agent wN\n") between stamp and re-stamp.
Actual: `Ok(DiskVersion(Expected{stamp: len 9, mtime == ctime, hash: ...}))`, assert at document.rs:1675.
Cause: `DiskVersion::of_path` (document.rs:~806) detects a change only through `DiskStamp` (len, mtime, ctime, dev, ino). The test's writes happen within the same kernel timestamp tick (coarse clock, 1 ms here: `stat` shows mtime advancing 1 ms per write) as the file creation, and length is the same, so the stamp is identical before/after and the loop accepts it. The returned version still pairs the stamp with the hash of the content read after the write, so it describes the current file; I found no lost-write path (strict racing tests 20/20 clean). It is a test that assumes the timestamp changes (fast hardware / coarse clock), or a documented limit of stamp-based detection. Needs a leader decision: make the test robust (e.g. vary length per write, or sleep > tick before the first hook) or accept the limit in decisions.md.

### Racing results (ledger warning: one check per write path)
| Write path | Racing test (vr_core::testutil / crate tests) | Result |
|---|---|---|
| save | `document_save_never_loses_a_concurrent_write`, `document_save_never_loses_a_write_from_a_fast_writer`, `document_save_race_leaves_whole_content`, `document_save_without_a_writer_writes_ours` | under the suite: pass; again 20x alone (testutil::tests::document_save, 4 tests/run): 20/20 pass, 0 failures (race20.txt). D76/D99 signature never seen |
| Save As | `save_as_seen_never_loses_a_*`, `unprompted_save_as_never_loses_a_concurrent_write` | pass under suite |
| explorer paste/move/trash | vr-fs tests/racing_writer.rs (2) | pass |
| project replace | vr-search tests/racing_writer.rs (2) | pass |

### Bench (scripts/bench-startup.sh, release, 2 runs)
first_frame_ms = 24 and 23 (target < 100, includes pty setup); max_rss_mb = 6 and 6 (target < 60).

### AC table (M1-M3 + AC-18; AC-13..AC-17 are M4-M8 = n-a)
| AC | Status | Evidence |
|---|---|---|
| AC-1 | pass | bench above; walk: no-arg start shows explorer tree and root path in title/welcome |
| AC-2 | pass | walk: `vr-ide main.rs` from src/ and `vr-ide /abs/src/main.rs` from /tmp both open src/main.rs, root = proj (git root), status "src/main.rs" |
| AC-3 | pass | walk: type "ZZ" -> tab "a.txt ●", ctrl+s -> disk `ZZalpha...`, marker cleared; undo -> text and marker back, redo -> ZZ; unit tests in vr-core pass |
| AC-4 | pass | walk with DISPLAY unset: ctrl+a ctrl+c emits OSC 52 `aGVsbG8K` (= "hello\n"); ctrl+v pastes, disk `hello\nhello\n`; clipboard unit tests pass |
| AC-5 | pass | walk: ctrl+s saves; keymap with unknown command shows `keymap: ctrl+k is bound to unknown command "no.such.command" (global) (+1 more)`, no crash |
| AC-6 | pass | walk: F1, "goto" lists "Go to File file.goto ctrl+e / ctrl+p" and "Go to Line nav.goto_line ctrl+g"; Enter runs (go-to-line prompt, "Line (1-3)") |
| AC-7 | pass | walk: unmodified buffer reloads 0.35 s after external write; modified buffer gets the "File Changed on Disk" Reload / Keep mine dialog; Esc keeps mine; unit test with real watcher passes |
| AC-8 | pass | walk: ctrl+n creates newfile.txt on disk; F2 renames (disk renamed, open tab and tree follow); Delete -> dialog (default Cancel by D104), Left+Enter -> file in `$XDG_DATA_HOME/Trash/files` + `.trashinfo`, original gone. Copy/move: vr-fs and app tests (not re-walked) |
| AC-9 | pass | walk: `ignored/` hidden in explorer and `x.txt` gives "No matching files" in go-to-file; project search for "fn " lists 5 results in 2 files, none from ignored/x.txt |
| AC-10 | pass | walk: regex `fn (\w+)` -> `fn new_$1`, palette "Replace All": 3 lines replaced, one ctrl+z restores all, disk unchanged after save |
| AC-11 | pass | walk: panel streams results and groups by file; 50k-file numbers from T9 QC (first batch 6 ms); release timing not re-run on a 50k tree |
| AC-12 | pass | walk: preview with per-file [x]/[ ] and diff lines; unchecked a.txt untouched on disk, checked main.rs written ("replaced 2"); open tab replaced in buffer only, disk untouched, ctrl+z restores |
| AC-18 | pass | release `open_100mb_under_2s` 0.42 s; frame 1.06 ms on 100 MB; 10 MB single line key+frame 224 us |
Other walk steps: quit guard (modified + ctrl+q -> Quit dialog listing a.txt, Esc cancels and app stays; ctrl+q + Enter saves ("ZZfn alpha") and exits; clean file quits at once, D128/D129 project-replace quit not re-walked, covered by unit tests passing); sizes 80x24, 120x40, 40x12 render without panic or overlap (explorer + editor + status visible; at 40x12 the notice banner takes rows as already noted in T10). Pty harness answered DA1 only, so the "terminal does not report these keys" notice shows in all walks (expected).

### Rule check sign-off
| Rule | Result | Evidence |
|---|---|---|
| AC-1..AC-12, AC-18 | pass | table above (AC-13..17 n-a) |
| BASE-25 guard | pass | guard.txt: fmt OK, clippy exit 0 |
| COST full suite once | pass | one `run-heavy.sh cargo test --workspace -j 4`; failing crate stopped cargo, remaining crates run once each, no whole rerun; failing test rerun 3x alone (deterministic) |
| Ledger: concurrent writer per write path | pass | racing table above; save 20x clean |
| Known flaky watcher test | n-o | passed in the suite |
| BASE-14/15 | pass | background tasks only, own pty children killed by pid (none left) |
| BASE-17 | pass | fixtures under qa-artifacts/signoff/proj only; config/state/xdg temp dirs removed |
| Defects | fail | 1 (test above) |

## QC FR-6
Scope: crates/vr-core/src/document.rs (worktree .worktrees/FR6). Script and raw output: qa-artifacts/FR-6/qc.sh, qc.txt (run through run-heavy.sh, 19 s).
| Check | Command | Result |
|---|---|---|
| Sign-off repro 5x | `cargo test -p vr-core --lib disk_version_of_a_file_that_keeps_changing` | 5/5 ok |
| Same-tick tests, once each | `... same_tick`, `... disk_version_in_one_tick` | 4/4 ok (save, save_overwrite_seen, save_as_seen, disk_version) |
| Strict racing tests 20x under 4 busy-loop CPU load | `testutil::tests::document_save`, `save_as_seen_never_loses`, `unprompted_save_as` (7 tests/run) | 20 pass / 0 fail |
| Independent same-tick probe | shell `stat` of back-to-back same-length writes | tick here is about 2 ms between writes (mtime moves); a same-tick rewrite cannot be forced from outside the process. The seams (`freeze_clock`, `no_exchange`) are cfg(test) only, so no entry point exists without editing code; relied on the in-tree tests, whose helper `rewrite_in_same_tick` asserts the stamp did not move (each test proves its own setup), plus the developer's red-before-fix lines. |
Note: the same-length keeps_changing test was changed to vary length (developer item (b)); assertions unchanged, mutation proofs in 04_dev_FR-6.md. Reviewer confirms.

## Rule check FR-6
| Rule | Verdict | Evidence |
|---|---|---|
| AC-3 save never loses/overwrites a concurrent write (save) | pass | save_never_replaces_a_write_in_the_same_tick_as_its_check + document_save racing 20/20 under load |
| AC-3 save_overwrite_seen (Keep mine) | pass | save_overwrite_seen_never_replaces_..._same_tick + racing |
| AC-3 save_as_seen / save_as | pass | save_as_seen_never_replaces_..._same_tick, save_as_seen_never_loses_*, unprompted_save_as racing 20/20 |
| AC-3 no false data loss | pass | racing and disk_version tests green; Expected equality ignores `racy`, so no spurious conflicts |
| AC-7 external change detection (vr-core paths) | pass | disk_version_in_one_tick test (version is of the bytes on disk), check_disk path untouched |
| Sign-off defect gone | pass | repro 5/5 |

## QC FR-6 r1
Script/output: qa-artifacts/FR-6/qc_r1.sh, qc_r1.txt (via run-heavy.sh).
| Check | Result |
|---|---|
| 5 `*_racing_the_end_of_the_window_stays_racy` tests, once | 5/5 ok |
| `disk_version_of_a_file_that_keeps_changing` 3x | 3/3 ok |
| Strict racing 20x under 4 busy-loop load (7 tests/run) | **18 pass / 2 fail**: `testutil::tests::document_save_never_loses_a_write_from_a_fast_writer` panicked at testutil.rs:402 (the "lost write" assert); the panic detail was cut by my grep and not captured |
| Follow-up (not requested, to size the flake) | same test alone under load 30/30 pass; full 3-filter set under load 40/40 pass, 0 fail (no panic output left to inspect) |
Round 0 (before the fix) gave 20/20 and 20/20 (developer and QC), so 2/20 is a new signal, not proven to be caused by the round-1 change: it may be the known rare D75(2)/D76(3) false positive (mode=InPlace, visible=false, saw_ours, replaced_is_ours), which the assert comment says is only tolerated informally. Verdict QC_FAIL on the strict-racing criterion until the signature is identified.

## QC FR-5
Engine: release binary target/release/vr-ide (built through run-heavy.sh, 49 s) driven through a pseudo-terminal (h.py; flows in qa-artifacts/FR-5/fr5.py, output in qa-artifacts/FR-5/out1.txt and the console). Related tests once: `cargo test -p vr-ide --lib -- explorer_tests:: search_tests:: commands::search::gate_tests::` -> 63 passed, 0 failed.

| # | Flow (release walk) | Result | Evidence |
|---|---|---|---|
| 1a | Save As src/new.rs while src is expanded | PASS | row new.rs appears under src, title becomes new.rs, file on disk |
| 1b | Delete the open src/main.rs externally (tab shows "(deleted)"), Ctrl+S | PASS | main.rs row gone after the delete, back after save; file recreated on disk |
| 2a | Search fn, rename a.txt -> renamed.txt in the explorer, reopen the search panel | PASS | results list renamed.txt, not a.txt; Search: Next Result works in renamed.txt |
| 2b | Replace preview containing a.txt, rename a.txt -> moved.txt | PASS | notice "files moved since the preview; Enter in the replace field previews again"; preview dropped; Enter previews again with moved.txt |
| 3 | Hidden off, reveal .github/workflows/ci.yml; reveal ignored target/out.log | PASS | nothing hidden expanded; ignored file: notice "target/out.log is hidden in the explorer (ignored by .gitignore); show ignored files with the command explorer.toggle_ignored"; with ignored/hidden on, reveal expands and selects. The dotfile notice was queued behind "hidden files hidden" ("+1 more") and not read as text; covered by explorer_tests::reveal_of_a_hidden_or_ignored_file_says_how_to_show_it |
| 4 | Replace fn->func over 300 files (1.2 MB each, ~9 s), files opened in tabs via Ctrl+P while it runs | PASS | f0250, f0200, f0150, f0100 opened mid-run: disk bytes unchanged (no "func"); result "4 files not replaced: many/f0250.txt (opened in a tab during the replace; not replaced); ..."; 296/300 replaced; files opened after their write show the replaced text; nothing corrupted |
| 5a | Open a.txt and b.txt, rename b.txt -> a.txt | PASS | rename refused, both files unchanged on disk |
| 5b | Close one of two tabs on one path, external reload in the other | NOT WALKABLE | state cannot be made through the UI any more; covered by explorer_tests::closing_a_tab_keeps_the_watch_of_another_tab_on_the_path (passes) |
| 6 | 39 columns, search panel full width: Esc and F6, then type XYZ | PASS | panel closes, explorer + editor visible, XYZ lands in the visible editor (Ln 1, Col 4); never into a width-0 editor. The script's own "no XYZ in a.txt" assertion printed FAIL; the screen shows the editor visible, so that assertion was wrong, not the product |

Regressions: AC-3, AC-8, AC-9, AC-12 behaved as expected in the flows above. Harness note: Go to File needs about 0.3 s after typing before Enter selects (harness timing, not product). The 300-file tree was deleted after the walk.

## Rule check FR-5
| Rule | Status | Evidence |
|---|---|---|
| BASE-14 | pass | no waiting loops; every command finished in the foreground |
| BASE-15 | pass | only the harness's own child processes were killed |
| BASE-17 | pass | the 300-file test tree deleted after the walk |
| BASE-19 | pass | flows recorded in this section |
| COST | pass | one release build, one related-test run, one flow script |
| Product code and tests | pass | not modified |

## QC FR-6 r2
Replay of the r1 check on the current worktree (no code change): the 3 filters (`testutil::tests::document_save`, `save_as_seen_never_loses`, `unprompted_save_as`) 20x under 4 busy-loop hogs, `--nocapture`, full output per run in qa-artifacts/FR-6/qc_r2/run_N.txt (failing runs would be fail_N.txt).
Result: 20 pass / 0 fail. Developer's r2 evidence checked: 7 fail-*.txt in qa-artifacts/FR-6/r2/, all 7 carry exactly `[mode=InPlace visible=false saw_ours=true replaced_is_ours=true]` (the known D75(2)/D76(3) false positive, D99); r1 4/300 vs r0 3/300 is not distinguishable, so the r1 change adds no new racing signature.
Withdrawn: the r1 racing failure was the known false positive (my r1 grep had cut its signature). Verdict QC_PASS.

## Re-sign-off (19:40)
Commit tested: HEAD 9133968 (master), clean product tree. Artifacts: `qa-artifacts/resignoff/` (suite.txt, guard.txt, ignored.txt, race20.txt + race_N.txt, walk.py/h.py, out.txt (walk on a stale 19:05 binary, discarded), out2.txt (final walk on the fresh release binary built 19:29 from HEAD), save3.txt, bench.txt). Engine: release binary under a python pty (no browser/Flutter).

Verdict: SIGNOFF_PASS.

### Tests (debug, once: `run-heavy.sh cargo test --workspace --no-fail-fast -j 4`, EXIT=0)
| Crate | Passed | Failed | Ignored |
|---|---|---|---|
| vr-config (unit 3 + tests/config 16) | 19 | 0 | 0 |
| vr-core (lib) | 151 | 0 | 1 |
| vr-fs (lib 43 + racing_writer 2) | 45 | 0 | 1 |
| vr-search (lib 37 + racing_writer 2) | 39 | 0 | 1 |
| vr-ide (lib 170, bin 0) | 170 | 0 | 3 |
Doc tests 0 everywhere. Total 424 passed, 0 failed, 6 ignored (sign-off 1 had 400 passed / 1 failed).
Guard: `cargo fmt --all --check` OK; `cargo clippy --workspace --all-targets -- -D warnings` exit 0 (guard.txt).
Ignored timing tests, release, once: vr-core `open_100mb_under_2s` ok (0.41 s); vr-ide `long_line_keystroke_to_frame_fast`, `render_large_file_fast`, `find_in_100mb_file_stays_responsive` ok (3/3).

### Racing (strict, 20x, 7 tests per run: testutil::tests::document_save*, save_as_seen_never_loses*, unprompted_save_as*)
20/20 runs exit 0, 0 panics (race20.txt). The D75(2)/D76(3) signature never appeared; no unclassified failure.

### Walk on the release binary (out2.txt)
| Flow | Result | Evidence |
|---|---|---|
| AC-3 save | pass | type ZZ, ctrl+s -> disk `ZZalpha fn` |
| AC-7 external change + Keep mine | pass | "File Changed on Disk" dialog; Esc keeps mine; ctrl+s asks again ([Keep mine] [Reload] [Cancel], Enter = Cancel, safe default); Keep mine -> disk `ZZYYalpha fn` |
| Save As (AC-3) | pass | src/new.rs row appears, file on disk |
| AC-8/AC-9 explorer rename + project search | pass | search results list renamed.txt after rename; Next Result and reopen work |
| AC-12 replace preview + rename | pass | notice "files moved since the preview; Enter in the replace field previews again", moved.txt re-previewed |
| AC-12 replace with tabs opened mid-run (300 files) | pass | f0290 opened at 1.26 s, f0250 at "Replacing 284/302"; further opens during/after; all 300 files replaced on disk, 0 files still contain the old text, open tabs show the replaced text, no corruption. Note: the replace now finishes in about 2.5 s (FR-5 run: 9 s), so the "opened in a tab; not replaced" skip path was not triggered in this run; it stays covered by the FR-5 walk and unit tests |
| AC-10 find bar | pass | ctrl+f "fn" shows 1/3, Enter/F3 reaches 3/3, Esc closes, typing goes to the editor |
Earlier evidence for AC-1, 2, 4, 5, 6, 11, 18 is unchanged (flows untouched by FR-5/6/7).

### Bench (scripts/bench-startup.sh, release, once)
first_frame_ms = 26 (target < 100), max_rss_mb = 6 (target < 60).

## Rule check re-sign-off
| Rule | Result | Evidence |
|---|---|---|
| AC-1..AC-12, AC-18 | pass | walk above + earlier sign-off evidence for untouched flows |
| BASE-25 guard | pass | fmt OK, clippy exit 0 |
| COST full suite once | pass | one workspace run, --no-fail-fast, 424/0 |
| Ledger concurrent writer per write path | pass | racing 20/20 clean; vr-fs and vr-search racing_writer in the suite |
| Sign-off 1 defect (vr-core disk_version test) | pass | vr-core 151/0 |
| FR-7 load-flaky find test | pass | passed in the suite (vr-ide 170/0) |
| BASE-14/15 | pass | background tasks only, own pty children killed by the harness |
| BASE-17 | pass | fixtures only under qa-artifacts/resignoff; no data in shared stores |
| Defects | none | |

## QC FR-8
Fresh release build (run-heavy, binary mtime 19:35:43 after build start 19:35:14). Script: qa-artifacts/FR-8/fr8.py (flows keepdel, dotsave, openrace_gt, openrace_ex), engine: pty harness on the release binary.

| Flow | Result | Evidence |
|---|---|---|
| 1 Keep mine recreates a file deleted while the prompt is open | pass | type ZZ, external write, Esc, ctrl+s opens the overwrite prompt, file deleted (row drops, "(deleted)"), Keep mine #1 refused ("changed again on disk; nothing was replaced"), Keep mine #2: row a.txt back in the explorer, disk = 'ZZalpha fn\n' |
| 2a dotfile save, hidden off | pass | two saves of .gitignore: disk has both edits, row stays hidden, no notice or flicker; re-read count covered by explorer_tests::saving_a_filtered_file_does_not_re_read_the_folder |
| 2b toggle hidden | pass | "hidden files shown": .gitignore row appears |
| 3a/3b open of a file mid-write (explorer, go-to-file) | not reproducible live | 4 attempts each with 40 files x 25 MB and reverse-order open bursts: the replace finished before the UI processed the opens (screen showed "Replaced 10002 matches in 42 files" while Go to File was still open); the per-file write window is milliseconds. Covered by commands::search::gate_tests::being_replaced_refusal_is_a_warning_on_every_path (passes: Warning severity on explorer, go-to-file and search panel paths; wording "try again in a moment") |

Tests: `cargo test -p vr-ide --lib -- explorer_tests:: dialog_tests:: commands::search::` 70 passed / 0 failed; fmt check OK; clippy --workspace --all-targets -D warnings exit 0.
AC-3 (save, recreate), AC-7 (keep mine prompt behaviour unchanged), AC-8 (explorer rows), AC-12 (replace tests incl. concurrent-writer tests in dialog_tests) show no regression.
Fixtures deleted after the walk (FR-8 dir 360K).

## Rule check FR-8
| Rule | Result | Evidence |
|---|---|---|
| FR-8 (1) keep mine row + bytes | pass | flow 1 |
| FR-8 (2) dotfile save, no re-read, toggle shows | pass | flows 2a/2b + unit test |
| FR-8 (3) warning notice "try again in a moment" on all open paths | pass (unit) / live not reachable | gate_tests; live race window too short, see above |
| AC-3, AC-7, AC-8, AC-12 | pass | flows + 70 tests |
| BASE-25 guard | pass | fmt OK, clippy 0 |
| BASE-14/15/17 | pass | no background tasks, fixtures under qa-artifacts/FR-8 removed |
| Defects | none | |

## QC FR-10
Engine: pty harness (qa-artifacts/FR-10/fr10.py, fr10b.py), TERM=xterm-256color 80x24, legacy and kitty (answers CSI ? 0 u). Binary target/release/vr-ide (mtime 21:01:44, `cargo build --release` Finished, no source newer than the binary).
Results (qc_legacy_show.txt, qc_kitty_show.txt, qc_extra.txt; the non-SHOW runs of the old section are void: tree hidden by design, harness crashes on click):
- (1) HIDDEN explorer at start-up: both modes.
- (2) ctrl+e then Down Right: WORKS both modes; application-cursor ESC O B / ESC O C: WORKS both. Enter opens file, Home, shift+Down, ctrl+w: WORKS.
- (3) ctrl+e shows, ctrl+e hides (typing "Q" reaches b.txt), ctrl+e shows again, arrows move: WORKS both.
- (4) ctrl+e from find bar: hides (bar stays), editor focus; show again + arrows: PASS both. From search panel (kitty, ctrl+shift+f): explorer shown, arrows work: PASS. With a file open: PASS (flow above).
- (5) ctrl+p opens Go to File: PASS; ctrl+b toggles: PASS; ctrl+b then F6 focuses tree: PASS; mouse click root row: WORKS both (tree shown).
- (6) legacy ctrl+shift+e = ctrl+e: from hidden shows + focuses (WORKS); from a shown tree hides it. Kitty ctrl+shift+e: focus, WORKS. Esc / palette close return to the editor (D100, unchanged, listed as "FAILS" by the old scenario by design). ctrl+b shows without focus (unchanged by design).
- AC-8: Enter opens b.txt from the tree, F6 back, Home: WORKS both.
Tests: `cargo test -p vr-ide --lib -- explorer_tests:: app_tests::` 65 passed, 0 failed; `cargo test -p vr-config` 3 + 16 passed.
Notes (not defects): a shown tree + legacy terminal: ctrl+shift+e hides it (D143); a startup notice lists unreported keys incl. ctrl+shift+e.

## Rule check FR-10
| rule | status | evidence |
|---|---|---|
| AC-8 | pass | keyboard tree use incl. Enter-open, both terminal modes |
| D100 | pass | Esc/Enter behaviour unchanged in replay |
| D101/D142 | pass | hidden at start, amended per user |
| D143/D144 | pass | find bar / search panel / legacy ctrl+e checked |
| USER-1 | pass | ctrl+e override logged (D142) |
| BASE-14/15/17 | pass | background run, nothing left running, no data probes |

## QC T14 — read mode / edit mode (QC_PASS)
Engine: own pty harness (qa-artifacts/FR-10/h.py + qa-artifacts/T14/qc_flows.py), TERM=xterm-256color, release build (binary 21:34, newer than all product sources; `cargo build --release` = Finished, no rebuild needed), legacy and kitty (flags answered to the `CSI ? u` query). One run per mode, 14 flows each; outputs qc_legacy.txt, qc_kitty.txt.
Flows (both modes, all PASS): open from ctrl+p -> READ; from explorer (ctrl+e, Down, Enter) -> READ; tab switch (ctrl+pagedown, other tab was EDIT) -> READ and typing refused; F1 + Esc -> READ; Save As dialog + Esc -> READ; project search panel -> READ; blocked keys (typing, Enter, Backspace, Delete, ctrl+v, bracketed paste, ctrl+x, ctrl+z, ctrl+y, alt+up/down, ctrl+], ctrl+/, kitty ctrl+shift+k) each show "Read mode — ctrl+i to edit", buffer and saved bytes unchanged; replace-all via palette from the open replace bar: hint shown, buffer/disk unchanged (script reported FAIL only because my assertion matched the "Replace ZZZ" input row; screen dump shows hello/world unchanged — false negative); ctrl+i (Tab in legacy) -> EDIT, edit + ctrl+s + undo + redo each verified on disk; Esc -> READ and clears selection; Esc closes find bar and palette; ctrl+n file opens EDIT; cursor CSI 2 SP q (read) / 5 SP q (edit) / 0 SP q on quit; badge at 80x24 and 40x12.
Legacy terminals do not report ctrl+shift+* keys, so dialog/search/replace-all were reached through the palette in both modes.
Not walked in pty: project replace into an open READ tab — covered by `search_tests::project_replace_preview_and_confirm` (+8 project_replace tests, 9/9 ok).
Tests: `cargo test -p vr-ide --lib mode_tests::` 21 passed, 0 failed.
Defects: none.

## Rule check T14
| Rule | Result | Evidence |
|---|---|---|
| BASE (QC: no product edits, no git writes) | ok | only files under qa-artifacts/T14 and this report touched |
| BASE-14/15 | ok | one background task, ended by notification, nothing killed |
| BASE-17 | ok | probes only in mktemp dirs, no shared DB/app |
| Decisions D145, D149-D155 | ok | flows above |

## QC M4-2 — highlighter engine (library) — QC_FAIL
Engines: cargo test (debug + release), throwaway binary (Rust, depends on vr-syntax by path). No browser (library task). Logs in qa-artifacts/M4-2/qc/.

### Commands and results
- `cargo test -p vr-syntax -p vr-config` once, debug (debug-once.log): all green — vr-config 3+16+7, vr-syntax unit 18, grammars 23, highlight 20 passed / 3 ignored, registry 5.
- Same suite 5x with 4 `timeout 900` busy loops (stress.sh, stress-1..5.log): runs 1-4 green; **run 5 FAILED**: `worker_never_blocks_the_caller_on_a_5mb_file` — `5 MB: initial parse landed after 11.2s`, assertion at tests/highlight.rs:383 `left: Disabled(Timeout { after: 10s }) right: Ready`. Unloaded debug parse is ~7.3 s, default parse_timeout 10 s, so under CPU load (4 loops + other agents' heavy jobs on the machine) the debug 5 MB parse crosses the timeout. Flaky under load (1/5); the test couples to debug-build speed vs the 10 s default.
- Release ignored perf tests (perf-release.log), 3 passed, vs developer table:
  | Case | QC initial | QC edit→spans avg/worst | dev avg/worst |
  |---|---|---|---|
  | 200 KB Rust | 79 ms | 14.4 / 24.6 ms | 10.4 / 16.8 |
  | 200 KB HTML+script/style | 198 ms | 22.8 / 28.0 ms | 22.0 / 26.6 |
  | 5 MB Rust | 1.61 s | 298 / 374 ms | 327 / 374 |
  | 10 MB one-line JS | 4.69 s | 1.24 / 1.51 s | 1.24 / 1.53 |
  spans() worst: 5.8 / 2.8 / 2.0 / 0.54 ms. Consistent with the developer's numbers.
- Independent check (src/main.rs, independent.log, full-waiteach.log): 20 samples, each Ready, every language >= 3 distinct classes (min json 4, yaml 6, markdown/markdown_inline 5; others 7-11) — see table below. AC-13e and size limit: PASS (texts below).
- 200 seeded random single-char edits per sample (LCG, insert/delete/replace, includes newline, quotes, `<`, `{`, `` ` ``, é, 中), fresh Highlighter on the final text compared at every 10th edit and at the end: 18 of 20 samples equal. **Mismatches in blade.blade.php and markdown.md**, reproducible and deterministic: with the check after every edit (`WAIT_EACH=1`), markdown.md differs from step 3 on, blade.blade.php from step 115; with checks every 10 edits markdown differs at steps 10-60 (and heals later), blade matched.
  Example (blade, step 115, edit 30..31 delete, `DUMP=1 WAIT_EACH=1 ./target/release/qc-m4-2`): incremental spans lack `(10..32, Comment)` that the fresh parse has; text `é{{{-- Li#ts <the) … --}}\n…`. Markdown: 16 spans incremental vs 19 fresh at step 3 (full dump in full-waiteach.log). Status is Ready in both, so the stale/missing colour is silent. Developer's random-edit test passes for these languages because its snippet-based edits do not hit this case; AC-13b "colours correct for the changed region" fails for these.

### Per-language class table (spans per class, sample file, Ready)
blade.blade.php lang=blade ready=true spans=42 distinct=8 OK	 {"Attribute": 2, "Comment": 2, "Operator": 4, "Other": 4, "Property": 2, "Punctuation": 16, "String": 2, "Tag": 10}
css.css lang=css ready=true spans=46 distinct=11 OK	 {"Attribute": 1, "Comment": 1, "Function": 1, "Keyword": 1, "Number": 4, "Operator": 3, "Property": 6, "Punctuation": 22, "String": 2, "Tag": 2, "Type": 3}
dart.dart lang=dart ready=true spans=49 distinct=8 OK	 {"Comment": 1, "Function": 5, "Keyword": 6, "Other": 3, "Property": 1, "Punctuation": 24, "String": 4, "Type": 5}
go.go lang=go ready=true spans=26 distinct=7 OK	 {"Comment": 1, "Function": 5, "Keyword": 7, "Operator": 1, "Other": 5, "String": 5, "Type": 2}
html.html lang=html ready=true spans=60 distinct=10 OK	 {"Attribute": 2, "Comment": 1, "Constant": 1, "Function": 1, "Number": 1, "Property": 1, "Punctuation": 34, "String": 3, "Tag": 15, "VariableBuiltin": 1}
javascript.js lang=javascript ready=true spans=52 distinct=11 OK	 {"Comment": 1, "Function": 4, "Keyword": 6, "Number": 4, "Operator": 4, "Other": 5, "Property": 2, "Punctuation": 21, "String": 1, "Type": 2, "VariableBuiltin": 2}
json.json lang=json ready=true spans=11 distinct=4 OK	 {"Constant": 2, "Number": 1, "Property": 5, "String": 3}
markdown.md lang=markdown ready=true spans=19 distinct=5 OK	 {"Function": 1, "Keyword": 2, "Other": 1, "Punctuation": 12, "String": 3}
markdown_inline.md lang=markdown_inline ready=true spans=14 distinct=5 OK	 {"Constant": 1, "Function": 1, "Other": 2, "Punctuation": 9, "String": 1}
php.php lang=php ready=true spans=36 distinct=10 OK	 {"Comment": 1, "Constant": 1, "Function": 2, "Keyword": 5, "Operator": 3, "Other": 3, "Punctuation": 9, "String": 4, "Tag": 6, "Type": 2}
php_only.php lang=php_only ready=true spans=20 distinct=7 OK	 {"Comment": 1, "Function": 2, "Keyword": 5, "Operator": 3, "Other": 3, "String": 4, "Type": 2}
python.py lang=python ready=true spans=46 distinct=11 OK	 {"Comment": 1, "Constant": 1, "Function": 5, "Keyword": 9, "Number": 2, "Operator": 5, "Other": 9, "Property": 5, "Punctuation": 2, "String": 3, "Type": 4}
rust.rs lang=rust ready=true spans=71 distinct=11 OK	 {"Comment": 2, "Constant": 1, "Function": 5, "Keyword": 9, "Operator": 3, "Other": 2, "Property": 3, "Punctuation": 34, "String": 3, "Type": 7, "VariableBuiltin": 2}
scss.scss lang=scss ready=true spans=53 distinct=11 OK	 {"Attribute": 1, "Comment": 1, "Function": 2, "Keyword": 2, "Number": 3, "Other": 6, "Property": 6, "Punctuation": 26, "String": 2, "Tag": 1, "Type": 3}
toml.toml lang=toml ready=true spans=27 distinct=8 OK	 {"Comment": 1, "Constant": 1, "Number": 1, "Operator": 4, "Property": 8, "Punctuation": 4, "String": 2, "Type": 6}
tsx.tsx lang=tsx ready=true spans=38 distinct=10 OK	 {"Attribute": 2, "Comment": 1, "Keyword": 5, "Operator": 3, "Other": 3, "Property": 1, "Punctuation": 15, "String": 2, "Tag": 2, "Type": 4}
twig.html.twig lang=twig ready=true spans=41 distinct=8 OK	 {"Comment": 1, "Function": 2, "Keyword": 5, "Operator": 2, "Other": 5, "Punctuation": 19, "String": 1, "Tag": 6}
typescript.ts lang=typescript ready=true spans=46 distinct=10 OK	 {"Comment": 1, "Function": 1, "Keyword": 7, "Number": 1, "Operator": 5, "Other": 5, "Property": 3, "Punctuation": 13, "String": 1, "Type": 9}
vue.vue lang=vue ready=true spans=62 distinct=11 OK	 {"Attribute": 4, "Comment": 1, "Function": 2, "Keyword": 3, "Number": 1, "Operator": 5, "Other": 4, "Property": 2, "Punctuation": 29, "String": 3, "Tag": 8}
yaml.yaml lang=yaml ready=true spans=20 distinct=6 OK	 {"Comment": 1, "Constant": 1, "Number": 1, "Property": 6, "Punctuation": 8, "String": 3}

### AC-13e / AC-13c texts (real output)
- languages.toml `[klingon] grammar="does_not_exist"`: `languages.toml: [klingon] uses grammar "does_not_exist", which is not compiled into this build; its files open as plain text`; a.kl maps to klingon, Highlighter status `Disabled(NoGrammar)`, 0 spans: `no grammar for klingon in this build; shown as plain text`. Unknown language id: same Disabled(NoGrammar), no panic.
- 10000 B Rust with max_bytes 5000: `Disabled(TooLarge{10000,5000})`, `file is larger than max_highlight_bytes (10000 > 5000 bytes); shown as plain text`, 0 spans; 100 B under limit: Ready.

### Verdict per criterion
AC-13 pass (all 20 samples >= 3 classes); AC-13a covered by developer tests (green, not re-derived); **AC-13b FAIL** (incremental != fresh for markdown and blade); AC-13c pass; AC-13d pass (test), but test flaky under load; AC-13e pass.

### Defects
1. Incremental reparse yields different spans than a fresh parse (markdown.md from edit 3; blade.blade.php edit 115; Blade comment `{{-- --}}` colour lost). Repro: `cd qa-artifacts/M4-2/qc && WAIT_EACH=1 DUMP=1 ./target/release/qc-m4-2` (source src/main.rs, seed 0xC0FFEE ^ file_index*7919). Expected equal to fresh; actual missing/extra spans, status Ready. Likely injection-layer/range reuse (markdown_inline and blade layers).
2. Flaky `worker_never_blocks_the_caller_on_a_5mb_file` under load (1 of 5 stress runs, debug parse 11.2 s > 10 s timeout). Suggest the test use a larger parse_timeout; engine behaviour (Disabled::Timeout) itself is as designed.

## Rule check M4-2
| Rule | Status | Evidence |
|---|---|---|
| BASE-4 evidence | pass | logs above |
| BASE-5/COST heavy via run-heavy | pass | all cargo runs through run-heavy.sh |
| BASE-14 no polling | pass | stress run as background task, turn ended |
| BASE-15 own tasks only | pass | busy loops were `timeout 900` children, killed by PID in stress.sh |
| BASE-17 no data committed | n-a | no DB/app |
| BASE-18 no sub-agents | pass | |
| BASE-24 no git writes / no product edits | pass | throwaway crate only under $TEAM_WS/qa-artifacts/M4-2/qc (own target dir, `[workspace]` detached) |
| AC-13b | FAIL | defect 1 |
| AC-13d | pass with flaky note | defect 2 |
| AC-13/13a/13c/13e | pass | above |

Addendum (full-waiteach.log, checking after every edit): mismatching checkpoints per file — markdown.md 69, markdown_inline.md 3, blade.blade.php 2, rust.rs 3, twig.html.twig 2 (other 15 samples 0). So defect 1 also hits plain Rust, not only injection languages.

## QC M4-2 r1 — QC_FAIL
Logs in qa-artifacts/M4-2/qc/ (r1-*). The throwaway binary now takes `SEED` (default 0xC0FFEE); it was rebuilt against the current worktree (path dependency).

- Defect 2 (flaky 5 MB test): FIXED. 5 runs of `worker_never_blocks_the_caller_on_a_5mb_file` under 4 busy loops all passed (parse landed 6.1-7.5 s). `cargo test -p vr-syntax -p vr-config` once under the same load: all green (vr-config 3+16+8, vr-syntax 21, grammars 23, highlight 24 passed / 3 ignored, registry 6).
- Defect 1 (incremental != fresh), `WAIT_EACH=1`, 200 edits per sample:
  - default seed: 20/20 samples 0 mismatches (r1-waiteach-default.log). The earlier markdown/blade/rust/twig/markdown_inline defects are fixed.
  - second seed `SEED=987654321` (r1-waiteach-987654321.log): **php_only.php: 4 mismatching checks** (edits 142-145, status Ready on both, 14 vs 13 spans). Other 19 samples 0.
    Repro: `cd qa-artifacts/M4-2/qc && SEED=987654321 DUMP=1 WAIT_EACH=1 ./target/release/qc-m4-2` (dump in r1-php_only-mismatch.txt). Edit 142 inserts a backtick at char 37 of the php_only sample; the incremental result keeps a stale `(138..141, Function)` that the fresh parse of the same text does not have.
- No product code or tests edited; busy loops were own `timeout 900` children.

Rule check M4-2 r1: BASE-14, BASE-15, BASE-24, COST (run-heavy.sh) pass; AC-13b FAIL (php_only, seed 987654321); AC-13d pass.

## QC M4-2 r2 — QC_PASS
Logs in qa-artifacts/M4-2/qc/ (r2-*). Binary rebuilt against the current worktree.
- `WAIT_EACH=1`, 20 samples x 200 edits, seeds 987654321, default (0xC0FFEE), 1122334455, 271828182, 777000111: every run exit 0, 0 MISMATCH lines, 20/20 samples `mismatches=0`, `FAILURES: 0`. php_only seed 987654321 now equal to a fresh parse.
- `cargo test -p vr-syntax --test highlight` once under 4 busy loops: `25 passed; 0 failed; 3 ignored`, finished in 273.46 s.
- Observation (not a failure): the same file took 31.7 s under the same load in r1 and 12-17 s in round 0. The added from-scratch verify pass after error-containing cycles roughly multiplies debug-test time; the release perf numbers were not re-run in this round, so the edit-latency cost of the verify pass (shown as status Parsing until it lands) on multi-MB files was not measured by QC.
Rule check M4-2 r2: BASE-14, BASE-15, BASE-24, COST pass; AC-13b pass at library level (5 seeds + developer oracle).

## QC M4-3
Engine: pty (python harness with per-cell SGR tracking), fresh release build of the worktree (`target/release/vr-ide`, 19,596,688 B, built 01:36), TERM=xterm-256color; truecolor run with COLORTERM=truecolor, second run without it (256 colours). Legacy keyboard (ctrl+i = Tab). Script `qa-artifacts/M4-3/m43.py`; outputs `qc_truecolor.txt`, `qc_256.txt`. Method per language: count fg colours on screen that do not appear in a plain copy (.zzz) of the same file.

| Sample | truecolor new fg vs plain | 256 new fg vs plain |
|---|---|---|
| rust, python, javascript, typescript, tsx, css, scss, html, vue, php, blade, twig, go, dart, toml | 4-7 (PASS) | PASS |
| markdown, markdown_inline, yaml | 3 (PASS) | md 3, md_inline 2, yaml 2 (total fg 7 vs 5 plain) |
| json | 1 (total 5 vs 4): keys and punctuation take the plain colour (Capture::Property = 0xf6eefc, equal to text fg); strings/numbers one colour, true/null another | 0 |
| php_only.php | 0: sample has no `<?php`; `.php` maps to the php grammar so it is all HTML text (a vr-syntax unit-test sample, not a real file) | 0 |

Flows (truecolor: 46 pass / 3 fail on the first run; the 3 = json, php_only, and a script bug in Save As typing into the pre-filled name; the Save As and 5 MB checks were fixed in the script and rerun green):
- AC-13a PASS: html script/style, vue script/style, php code, markdown rust fence each show 2-5 distinct fg inside the embedded segment (same in 256).
- AC-13b PASS: ctrl+i, typing `// ` at line start turns line 1 to a single colour within 1.5 s; 6x ctrl+z restores 4 colours; newly typed `let y = "abc";` coloured.
- AC-13c PASS: max_highlight_bytes=100 via VR_IDE_CONFIG_DIR: notice "big.rs: file is larger than max_highlight_bytes (328 > 100 bytes); shown as plain text"; fg 5 vs 11 with the default limit; a file under the limit gives no notice.
- AC-13d PASS: 5 MB Rust: text on the first frame at 0.07-0.08 s with 4-5 fg (chrome only); colours (>=8 fg) land at 2.28 s (truecolor) / 2.27 s (256); typing echo latency max 53-60 ms, median 52-58 ms (pump granularity included).
- AC-13e PASS: `[klingon] uses grammar "klingon", which is not compiled into this build; its files open as ...` startup notice.
- Unknown extension: .txt and .zzz plain (1 fg), no notice. PASS.
- Reload after external change keeps correct colours (4 fg keyword/string line, comment single colour). PASS. Save As a.rs -> b.py: file written, tab shows b.py, `// note` now 2 fg (no longer a comment). PASS.
- `cargo test -p vr-ide --lib syntax_tests::`: 14 passed, 0 failed, 1 ignored.

Bench (AC-1, scripts/bench-startup.sh x3): first_frame_ms 24, 23, 23 (< 100); max_rss 5 MB (< 60). Binary size 19.6 MB.

Observations (not blocking): (1) JSON highlighting is thin by theme design: keys equal the text colour; only 2 colours beyond plain. (2) in 256 colours yaml / markdown_inline show only 2 colours not present in the plain screen (7 total fg) - palette fitting/baseline overlap, still coloured. 

Verdict: QC_PASS M4-3

## Rule check M4-3
| Rule | Result | Evidence |
|---|---|---|
| AC-13 | pass | per-language table; 2 observations above |
| AC-13a | pass | embedded segments coloured |
| AC-13b | pass | typing/undo flow |
| AC-13c | pass | notice text |
| AC-13d | pass | first frame 0.08 s, colours 2.3 s, echo <= 60 ms |
| AC-13e | pass | startup notice |
| AC-1 | pass | 23-24 ms, 5 MB |
| BASE-14/15/17 | pass | no polling loops, own tasks only, temp dirs only |
| BASE-18 | pass | no sub-agents |

## Sign-off after M4
Commit tested: master HEAD 8968a7b plus the leader's uncommitted workspace notes only; no product files modified. Artifacts: `qa-artifacts/signoff-m4/` (suite.txt, guard.txt, ignored.txt, race20.txt, race_N.txt, bench*.txt, size.txt, walk.py, w2.py, h.py). Engine: release binary under a python pty with a small VT model (no browser/Flutter).

Verdict: SIGNOFF_PASS.

### Tests (debug, once, run-heavy.sh cargo test --workspace --no-fail-fast -j 4, EXIT=0)
| Crate | Passed | Failed | Ignored |
|---|---|---|---|
| vr-config (unit 3, config 16, languages 8) | 27 | 0 | 0 |
| vr-core | 151 | 0 | 1 |
| vr-fs (lib 43 + racing_writer 2) | 45 | 0 | 1 |
| vr-search (lib 37 + racing_writer 2) | 39 | 0 | 1 |
| vr-ide (lib 270, bin 0) | 270 | 0 | 4 |
| vr-syntax (lib 24, grammars 23, highlight 26, registry 6) | 79 | 0 | 5 |
Total 611 passed, 0 failed, 12 ignored; doc tests 0.
Ignored, release, once (--workspace --release -- --ignored, EXIT=0): all passed: vr-core open_100mb_under_2s (0.44 s); vr-fs trash_child; vr-ide long_line_keystroke_to_frame_fast, render_large_file_fast, find_in_100mb_file_stays_responsive, highlighted_keystroke_to_frame_fast; vr-search project_search_first_batch_latency; vr-syntax perf_edit_to_spans_typical_files, _5mb_file, _10mb_single_line, perf_worker_cpu_while_typing_into_text_with_an_error, every_sample_equals_a_fresh_parse_full_seed_set (5/5, 42.6 s). No reruns needed.

### Guard
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings: EXIT=0 (guard.txt).

### Racing (strict, 20x, 7 tests per run)
20/20 runs exit 0, no panic; D75(2)/D76(3) signature not seen; nothing unclassified. vr-fs and vr-search racing_writer pass in the suite.

### Walk (release binary, pty 100x30, TERM=xterm-256color, legacy keys, COLORTERM=truecolor; fixtures outside the repo)
| Flow | Result | Evidence |
|---|---|---|
| FR-10 explorer hidden at start | pass | buffer only, no tree |
| FR-10 ctrl+e shows tree, arrows move in tree without typing into the buffer, ctrl+e hides | pass | tree rows a.txt/b.txt after ctrl+e; buffer unchanged; hidden again |
| T14 opens READ; typing refused; Tab to EDIT and typing works; Esc to READ; save | pass | status badge READ/EDIT; disk `Qhello\nworld\n` |
| T15 synthwave default theme (no config) | pass | truecolor bg 10;5;18 / 26;16;40, accents 255;46;151, 45;226;230, 255;157;60 |
| M4 highlight Rust / Vue / Markdown, edit + save | pass | distinct fg colours 12 / 13 / 9 vs 5 for plain text |
| FR-8 external change while dirty, Esc keeps mine, ctrl+s asks again, Keep mine | pass | disk `Yhello\nworld\n` |
| AC-9/AC-12 project search and replace with a tab open | pass | 3 results in 3 files; confirm dialog [Replace][Cancel]; "Replaced 3 matches in 3 files (1 in open tabs, not saved yet)"; open tab dirty with FUNC, its disk file unchanged until saved, other two replaced on disk |
Untouched flows (AC-2, 4, 5, 6, 11, 18) keep the earlier sign-off evidence. Probe note: the first walk run put fixtures under the repo so project search saw the whole repo; fixtures were moved outside it (probe artifact, not a product issue).

### Bench (scripts/bench-startup.sh, release, 2 runs)
Empty project: first frame 16 / 17 ms, max RSS 5 MB. 200 KB Rust file: first frame 17 / 18 ms, RSS at first frame 6 MB, max RSS after colours 17 / 18 MB. Targets (< 100 ms, < 60 MB) met. Binary 19,596,336 bytes (about 18.7 MiB).

### Defects
None.

## Rule check sign-off M4
| Rule | Result | Evidence |
|---|---|---|
| AC-1..AC-12, AC-18 regression on touched flows | pass | walk, earlier sign-off for untouched flows |
| AC-13 highlighting | pass | walk colours, vr-syntax 79/0 and 5 ignored incl. full seed set |
| FR-8, FR-10, T14, T15 | pass | walk |
| BASE-25 guard | pass | fmt OK, clippy exit 0 |
| COST full suite once | pass | one workspace run, 611/0 |
| Ledger concurrent writer per write path | pass | racing 20/20, racing_writer tests, Keep mine and project replace walks |
| Startup / RSS budgets | pass | 16-18 ms, 5-18 MB |
| BASE-14/15/17 | pass | background tasks only; only own processes touched; fixtures only under scratchpad and qa-artifacts |
| Defects | none | |

## QC M5-1 r1 (re-check, final)
Engine: cargo test + probe crate (qa-artifacts/M5-1/qc/src/main.rs, full.sh under timeout 1500, one background task, DONE written, own busy-loop PIDs killed). Outputs: qa-artifacts/M5-1/qc/r1/*.txt

| Run | Result |
|---|---|
| cargo test -p vr-term (quiet) | pass: lib 7, env 1, flood 1 (1 ignored), leak 1, pty 19 |
| loaded 1, 2, 4, 5 (4 busy loops) | pass |
| loaded 3 | FAIL: pty.rs::input_to_a_program_that_never_reads_is_bounded_and_never_blocks, panicked at pty.rs:309:5 "refused after 17 chunks" (18 pass, 1 fail) |
| release flood --ignored | pass: 100 MiB in 22.5 s; 6862 snapshots, median 626 us, p99 3.25 ms, max 11.7 ms; RSS 2.8 -> 32.5 MB |

Loaded-3 analysis: assertion i*64KiB <= MAX_PENDING_INPUT (1 MiB) saw 17 chunks (1.11 MiB). The cap counts queued bytes only (terminal.rs:96-101); the writer thread drains into the kernel tty buffer meanwhile, so accepted = cap + drained. Library bound held (InputBacklog returned, no blocking). Defect is the test assumption: flaky 1/5 under load, 0 quiet. Fix: add slack (about 256 KiB) or assert on pending_input().

Probe: htop alt_screen=true, 1469 coloured cells; resizes 80x24, 120x40, 60x20 reflected; after q alt_screen=false; stty size 20 60; ctrl+c gives 130; exit 7 gives Exited(7); /proc gone after exit and drop. AC-14d sleep 1000: shell and sleep gone 11.5 ms after drop. Disowned background job: threads 1->4->1, fds 4->8->4 (released); drop 1.3 ms; the stray sleep (pid 2585844) lingers detached, alive 2 s after drop (I stopped it by pid). Not an AC-14d failure.

## Rule check M5-1
| Rule | Status | Evidence |
|---|---|---|
| BASE-14 | pass | one bounded background script, no polling |
| BASE-15 | pass | stopped only own PIDs |
| BASE-17 | pass | no DB/app commits |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | outputs per run in r1/ |
| COST-13 | pass | throwaway only in qa-artifacts/M5-1/qc |
| No product edits | pass | probe crate and script only |
| AC-14b/c/d | pass | tests plus probe |
n-a: browser/SWEEP
Verdict: QC_FAIL (minor, test-only flaky assertion)

## QC M5-1 r2 (replay of the flaky input-cap test)
Script qa-artifacts/M5-1/qc/full2.sh (timeout 1500, own busy-loop PIDs killed, DONE written); outputs in qa-artifacts/M5-1/qc/r2/.
- Test change read in pty.rs:294-319: asserts the refusal (InputBacklog), write never blocks (< 1 s), pending_input() <= MAX_PENDING_INPUT after refusal, snapshot stays responsive.
- Loaded full sequence, `cargo test -p vr-term` x5 under 4 busy loops: 5/5 pass (every run exit 0, 6 suites ok each; the pty suite 19 passed).
- `cargo test -p vr-term --test pty input_to_a_program_that_never_reads` x10 under 4 busy loops: 10/10 pass.
- Busy loops confirmed gone.
Finding (flaky input-cap assertion) is closed. Rule check unchanged from M5-1 r1 (all pass). Verdict: QC_PASS.

## QC M5-2 (integrated terminal panel, TUI, mode QC, HEAD 4bb6bad)
Engine: real pty (python harness, TERM=xterm-256color COLORTERM=truecolor SHELL=/bin/bash, 100x30, temp project dir), fresh release build (binary mtime 04:36:55, built by run-heavy.sh in 43 s), once legacy (no kitty protocol, ctrl+` = NUL) and once with kitty protocol (CSI-u). Script: qa-artifacts/M5-2/qc/walk.py (lib.py, h.py); outputs qa-artifacts/M5-2/qc/legacy.txt, qa-artifacts/M5-2/qc/kitty.txt (screen excerpts included).

Tests (once): vr-config 3+17+8 ok; vr-ide lib 335 passed / 4 ignored, bin 0; vr-term 7 + env 1 + flood 1 (1 ignored) + leak 1 + pty 19, all ok. 0 failures.

Flow results (legacy 32/32, kitty 29/29; per-flow lines in the txt files):
- AC-1: no child process of vr-ide at start (/proc), welcome shows "ctrl+`        Terminal": PASS both.
- AC-14a: ctrl+` -> panel + TERM badge + 1 bash child, `pwd` = project root: PASS. kitty: ctrl+` again hides (welcome back): PASS. File open, ctrl+` -> panel focused, `echo hi` -> "hi" in panel, file unchanged on disk and in the editor: PASS. alt+q -> editor focused (badge gone, hint "ctrl+` focus"), ctrl+i, "Z" -> "Zhello" in the editor, disk still "hello": PASS. legacy: NUL inside a focused terminal does not hide, reaches the shell (`echo ok$((1+1))` -> ok2): PASS.
- AC-14: htop (SGR colours in output, alt screen: shell lines hidden, "Load average" visible), 3 resizes (26x90 badge 90x9, 34x120 badge 120x13, 22x70) htop redraws each time; q quits; `stty size` = "8 70" after 22x70, "11 100" after returning to 30x100 = badge: PASS. vi -u NONE: alt screen (~ lines), :q returns: PASS. ls --color=always emits SGR colours: PASS.
- AC-14b: exit -> "[process exited 0]"; Enter -> new shell works; 2nd tab (kitty ctrl+shift+`; legacy palette "New Terminal") badge 2/2, two bash children; "Focus Previous Terminal" -> 1/2 with tab 1 content: PASS.
- Scrollback: seq 1 3000, shift+pageup shows up to 2991 and indicator "↑ 10/2996" on the panel; typing returns to the bottom (3000): PASS.
- AC-14d: with `sleep 1000` running in 2 tabs: alt+q, ctrl+q -> "2 terminals still running; quitting ends them." [Quit anyway] [Cancel]; Enter (default Cancel) keeps everything; ctrl+q, Left, Enter -> then the unsaved-file dialog (file had a buffer edit; Save all/Discard/Cancel, as designed), Discard -> vr-ide exits, sleep and both shells gone (checked by PID in /proc), no leftover sleep process: PASS both modes.
- Status bar TERM badge while focused (title + "i/n · cols×rows"): PASS.

Observations (no QC_FAIL, for the leader):
1. Legacy terminals have no key that hides the panel. NUL in a focused terminal goes to the shell (spec), alt+q then NUL or the palette "Toggle Terminal" only FOCUSES the shown panel; the only way out is the palette "Close Terminal" (ends the shell, hides the panel). Consistent with the spec text and D206 but AC-14a's "ctrl+` hides" holds only in kitty. Worth a decision (for example palette/alt+q+toggle hiding when the editor already is focused, or a [terminal] alt key such as alt+`).
2. At 70 columns the status badge drops the "cols×rows" part (title is kept); cosmetic.
3. Legacy startup notice lists ctrl+shift+` among unreported keys (D206 says so, intended).

## Rule check M5-2
| Rule | Result | Evidence |
|---|---|---|
| AC-1 | pass | no child at start, welcome hint, /proc check |
| AC-14 | pass | htop/vi/ls colour, 3 resizes, stty size matches |
| AC-14a | pass (kitty full; legacy per D206, see obs. 1) | flows above |
| AC-14b | pass | exit banner, restart, tabs |
| AC-14d | pass | dialog default Cancel, Quit anyway, /proc PIDs gone |
| D8, D10, D205-D213 | pass | TERM badge, [terminal] keys, scrollback indicator observed |
| BASE-14/15/17 | pass | foreground runs only; only own pids killed (none needed); no data committed |
n-a: other BASE IDs (no production edits by QC)

## Sign-off after M5
Commit tested: master HEAD 8cffd8c (plus the leader's uncommitted workspace notes only; no product files touched). Artifacts: `qa-artifacts/signoff-m5/` (suite.txt, guard.txt, ignored.txt, walk.py/walk.txt, w40.py/w40.txt, bench.txt). Engine: release binary under a python pty with a small VT model.

Verdict: SIGNOFF_PASS.

### Tests (debug, once, run-heavy.sh cargo test --workspace --no-fail-fast -j 4, EXIT=0)
| Crate | Passed | Failed | Ignored |
|---|---|---|---|
| vr-config (3 + 18 + 8) | 29 | 0 | 0 |
| vr-core | 151 | 0 | 1 |
| vr-fs (43 + 2) | 45 | 0 | 1 |
| vr-search (37 + 2) | 39 | 0 | 1 |
| vr-ide (lib 344) | 344 | 0 | 4 |
| vr-syntax (24 + 25 + 26 + 6) | 81 | 0 | 5 |
| vr-term (7 + 1 + 1 + 1 + 19) | 29 | 0 | 1 |
Total 718 passed, 0 failed, 13 ignored; doc tests 0. Release ignored (once, EXIT=0): all 13 passed (core open_100mb, fs trash_child, 4 vr-ide perf, search first batch, 5 vr-syntax perf + full seed set 42.8 s, vr-term strict 100 MB flood 18.0 s). No reruns, no flaky.

### Guard
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings: EXIT=0.

### Walk (release, pty, TERM=xterm-256color, COLORTERM=truecolor, legacy keys, /bin/sh ENV=/dev/null)
| Flow | Result | Evidence |
|---|---|---|
| Welcome 120x40, 80x24: "ctrl+` Terminal" hint, no shell at start, numbers update | pass | START list with Terminal row; CPU/RAM/SWP/LOAD/UP/VR-IDE blocks; no children |
| Welcome 40x12 | pass (note) | CPU spark + RAM bar update over 3 s; 4 keys (go to file, palette, explorer, quit) by T17 design, Terminal is key 5 and shows from 40x13 on a clear screen; with the 15 s legacy-keys notice only 1 key shows (known, accepted in 05_review_T17_impl.md). My first check required the hint at 40x12, not a product defect |
| ctrl+` (NUL) opens, 1 sh child, echo hi output | pass | TERM badge, "hi" |
| NUL hides, NUL shows, shell survives | pass | badge empty then TERM again, pid alive |
| htop 2 s in panel | pass | Load average/Tasks visible |
| alt+q to editor | pass | badge not TERM |
| .xml opened, coloured | pass | 18 distinct truecolor fg values |
| .rs opened in READ, ctrl+i EDIT, type, ctrl+s | pass | disk `Zfn main() {` |
| explorer ctrl+e | pass | a.xml, m.rs listed |
| Quit with running terminal: dialog ("1 terminal still running; quitting ends it."), default Cancel keeps app, Quit anyway exits | pass | |
| No shell left after quit | pass | /proc check, none alive |

### Bench (scripts/bench-startup.sh, release)
First frame 18 ms, max RSS 6 MB; 200 KB Rust file first frame 20 ms, RSS 7 MB at first frame, 18 MB after colours; welcome with monitor 7 MB after 4 s. Targets met (< 100 ms, < 60 MB). Binary 20,224,752 bytes (about 19.3 MiB).

### Defects
None.

## Rule check sign-off M5
| Rule | Result | Evidence |
|---|---|---|
| Regression AC-1..AC-18 on touched flows (XML, T16, T17 welcome, M5 terminal) | pass | suite 718/0, walk |
| BASE-25 guard | pass | fmt + clippy EXIT=0 |
| COST full suite once, ignored release once | pass | one run each |
| Startup / RSS budgets | pass | 18-20 ms, 6-18 MB |
| No shell at start, none left after quit | pass | /proc |
| BASE-14/15/17 | pass | background task, only own PIDs, fixtures in /tmp |
| Defects | none | |

## QC W-1
Engine: release build (fresh, `cargo build --release -p vr-ide` Finished) driven in a pty (python harness `qa-artifacts/W-1/qc/flow.py` + `flows.py`/`flows3.py`, legacy keyboard, TERM=xterm-256color, SHELL=/bin/sh ENV=/dev/null, private VR_IDE_STATE_DIR / VR_IDE_CONFIG_DIR under qa-artifacts/W-1/qc/). One run per flow script, results:
- PASS AC-W1 go to file (ctrl+p) lists alpha.txt/deep.txt of a and beta.txt of b, each with its root name in the path column (`alpha.txt  a`, `deep.txt  a/sub`)
- PASS AC-W1 status bar `READ b/beta.txt`, `READ a/alpha.txt`
- PASS AC-W2 Search in Project: results in root order (a then b), a/skip.txt ignored by a/.gitignore, b/skip.txt found
- PASS AC-W2 Replace in Project: preview lists both roots, disk untouched until confirm; after confirm bytes on disk: a/alpha.txt "PIN one", a/skip.txt unchanged, b/beta.txt "PIN two", b/skip.txt "PIN b-not-ignored"
- PASS AC-W2 race (writer renaming a new beta.txt every 1 ms during confirm, 1232 writes): final file holds the writer's last version (WRITER1232); stale-preview check (file edited between preview and confirm): CONCURRENT-EDIT kept, the other root's file still replaced
- PASS AC-W4 `vr-ide x.vr-workspace` (`roots = ["a","b"]`) opens both roots; recent.toml after quit holds `[[workspace]] roots=[abs a, abs b]` (the workspace file is listed through its roots here, because the file entry is saved by Save As / Open; startup records the root set); corrupt (invalid UTF-8) recent.toml: warning "recent.toml: cannot read: stream did not contain valid UTF-8", app starts, file rewritten fresh
- PASS edge `a a/sub`: notice "a/sub is inside the workspace folder a; nested folders are not added", one root; `a a`: one root, no notice, no root column in go to file
- PASS AC-W6 New Terminal with b/beta.txt active: `pwd` = .../fx/b; with a/alpha.txt active: .../fx/a
- PASS AC-1 bench-startup x3: first_frame_ms=23, 22, 23 (< 100)
- PASS `cargo test -p vr-ide --lib workspace_tests::` 10 passed; `cargo test -p vr-config` 8 + 18 + 8 passed
- ~/.local/state/vr-ide untouched (checked listing).

Observations (not defects): search results show the root name as a trailing label (`alpha.txt a`) in root order rather than separate group headers; welcome screen shows the first root only (W-2 scope); a hand-written `.vr-workspace` needs `roots = [...]` (my first guess `[[folders]]` was refused with a clear error naming the file).

## Rule check W-1
| Rule | Status | Evidence |
|---|---|---|
| BASE-14 | pass | all runs foreground, no polling loops |
| BASE-15 | pass | only own pty children killed by PID |
| BASE-17 | pass | no data committed to shared DB; fixtures re-created per flow |
| BASE-19 | pass | results written at the end of one run per flow script (short) |
| COST-13 | pass | throwaway files only under qa-artifacts/W-1/qc/ |
| LEDGER writer-race | pass | one racing check on the replace write path plus a stale-preview check |

## T18
Engine: cargo (heavy wrapper `skills/project-profiling/references/run-heavy.sh`, PATH needs `~/.cargo/bin`), release build driven in a pty (python harness, legacy bytes, TERM xterm-256color, SHELL=/bin/sh, private VR_IDE_CONFIG_DIR / VR_IDE_STATE_DIR). Artifacts: `qa-artifacts/T18/qc/` (test.log, clippy.log, fmt.log, walk*.py, walk*_*.txt screens, docs_check.py/.out).

1. Suites: `cargo test --workspace` exit 0, 737 passed / 0 failed; `cargo clippy --workspace --all-targets -- -D warnings` exit 0; `cargo fmt --check` exit 0.
2. AC-5/AC-6 runtime (legacy bytes -> observed effect, screens `walk_*.txt`, `walk2_*.txt`):
- F1 (ESC O P) -> Command Palette with keys column (ctrl+w, ctrl+d, ...); Esc closes
- ctrl+t (0x14) -> SEARCH panel; ctrl+r (0x12) -> REPLACE panel
- ctrl+p + a.txt + Enter -> `READ a.txt`; tab (0x09, ctrl+i on legacy) -> `EDIT`; Esc -> `READ`; tab again -> `EDIT`
- ctrl+d (0x04): "alpha one" line removed; ctrl+l (0x0c): `beta` duplicated; ctrl+/ (0x1f): `// beta`
- ctrl+g (0x07) -> "Go to Line" dialog
- ctrl+u (0x15) -> Replace field; F7 (ESC[18~) replaced one (`ZZ one`); shift+F7 (ESC[18;2~) -> "Replaced 1 occurrence" (the rest); in read mode F7/shift+F7 do nothing (documented: edit mode only)
- F9 / shift+F9 (ESC[20~ / ESC[20;2~) -> b.txt / a.txt tabs switch
- ctrl+e toggles explorer shown/hidden; ctrl+b focuses explorer
- NUL (ctrl+`) -> `TERM sh 1/1` panel focused; NUL again -> editor `READ`; ctrl+F12 (ESC[24;5~) -> `2/2`; F8 (ESC[19~) -> `1/2`; ctrl+F4 (ESC[1;5S) -> terminal closed (panel shows `1 sh`)
- F12 (ESC[24~) -> "Save As" dialog; F5 (ESC[15~) on a dirty buffer -> reload notice, no crash
- Note (by D8, not a defect): terminal keys ctrl+F12/F8/ctrl+F4 act from the editor; in a focused terminal they go to the shell.
3. User keymap `"ctrl+k ctrl+s" = "file.save"`: vr-ide starts, notice shown: `.../keymap.toml: key "ctrl+k ctrl+s" is not a valid key chord: unknown modifier "k ctrl"` (InvalidKey; unit test `a_key_sequence_is_an_invalid_key` also passes). Note: the message text says "unknown modifier" rather than "key sequences are not supported"; cosmetic.
4. Docs: `docs_check.py` compares 61 bindings of keymap.toml with docs/keymap.md and README.md: every binding matches; the 4 reports are false positives (README writes `esc`; docs list keyless palette commands file.save_overwrite, explorer.toggle, explorer.move on purpose). No leader/alt/ctrl+shift default key in docs; the only ctrl+shift mention is the built-in terminal paste `ctrl+shift+v` (pre-existing, terminal-internal, not a vr-ide binding in keymap.toml).

Verdict: PASS. Defects: none (S4 cosmetic: InvalidKey wording for sequences, crates/vr-config/src/keymap.rs:234).

## Rule check T18
| Rule | Status | Evidence |
|---|---|---|
| AC-5 (one key per command, legacy-reachable) | pass | 25 keys driven with legacy bytes, effects observed; docs_check 61 bindings |
| AC-6 (sequence = InvalidKey, no leader/chord) | pass | startup notice with sequence keymap; no leader in docs/keymap |
| Full suite / clippy / fmt | pass | 737 passed, clippy -D warnings 0, fmt 0 |
| BASE-14/15 | pass | foreground runs, only own pty children killed |
| BASE-17 | pass | fixtures in /tmp, private config/state dirs, ~/.config not touched |
| BASE-19 | pass | results written to this report after the walks |

## T21 — keyset v2 (QC)

Verdict: QC_PASS. Engine: release binary driven through a python pty (TERM=xterm-256color), legacy bytes and kitty-encoded keys; cargo for the suites. Artifacts: qa-artifacts/T21/qc/ (walk.py, docs_check.py, legacy_*.txt / kitty_*.txt screens, run.txt, test_full.txt, clippy.txt, fmt.txt, f3only_*.txt).

- cargo test --workspace: 771 passed, 0 failed (test_full.txt). clippy --all-targets -D warnings: clean. fmt --check: clean. Fresh release build ok.
- Own pty walk (walk.py), 48 checks per run: legacy 48/48, kitty 48/48 (after fixing my script's kitty bytes for f3/shift+f3; f3only.py shows kitty shift+f3 steps back, legacy shift+f3 does not = D264, known).
- Covered both modes: space x hint, space space and f1 palette, tab cycle editor<->explorer, tab with find bar open (field unchanged), space i, edit tab / shift+tab, ctrl+l/d, ctrl+up/down, ctrl+a/x/v, ctrl+z/y, ctrl+/, esc, ctrl+r in editor (READ -> EDIT, replace field, one then all gives 1 then 2 OMEGA), ctrl+r on a non-UTF-8 read-only file (refused, stays READ, typing and disk unchanged), ctrl+r in search panel (project REPLACE), ctrl+h hidden toggle, explorer ctrl+n File/Folder (folder and file created on disk), explorer copy and cut/paste of a folder (recursive copy keeps source; cut moves), f2, delete, f3. Removed keys inert: f5 f12 ctrl+t ctrl+u f7 f4 f6 ctrl+f1 ctrl+f2 ctrl+f3 ctrl+f5 ctrl+i ctrl+] (no edit, no panel, no file change).
- docs_check.py: every keymap.toml binding has a docs/keymap.md row with the same key and appears in README; no removed key listed as a binding (docs/keymap.md:33 mentions ctrl+i only to say it is not an edit key; README f1 is written `F1`). Existing guard test config.rs passes in the suite.
- Known, not findings: D264 (legacy shift+f3), D265.
- Notes (info): the status bar does not show which pane has focus, so focus was probed by cursor movement.

## Rule check T21
| Rule | Result | Evidence |
|---|---|---|
| USER-5 keys work without kitty (ctrl+letter / F keys, 2 approved exceptions) | pass (D264 known for shift+f3) | legacy walk 48/48 |
| AC-21a each key runs its command, with and without kitty | pass | legacy 48/48, kitty 48/48 |
| AC-21b space i / space space / tab / edit tab / esc | pass | walk screens |
| AC-21c ctrl+r in-file and project | pass | walk screens, read-only refusal |
| AC-21d removed keys inert, docs match keymap | pass | walk + docs_check.py |
| Tests / clippy / fmt | pass | 771 passed, clippy -D warnings, fmt clean |
| Test data cleanup | pass | walk uses mkdtemp dirs only, no repo or DB data |

## T20 — slash commands phase 1 (QC, after review round 1)
Engine: python pty walk on the fresh release binary (TERM=xterm-256color, legacy bytes) + cargo. Artifacts: qa-artifacts/T20/qc/ (qwalk.py, qwalk.txt, q_NN.txt screens, fmt/clippy/full-suite/release txt).

Evidence
- fmt --check clean; clippy --workspace --all-targets -D warnings exit 0; full suite via run-heavy.sh: 26 result lines all ok, 816 passed, exit 0; release build exit 0.
- Docs table: test docs_table_matches_the_registry ok; script parse of the docs table finds 20 rows = 20 registry commands.
- qwalk: 54 checks, 53 PASS; the one FAIL was my own check (list shows 12 of 20 rows at 30 rows tall, `/help` is below the fold; list scrolls) — not an app defect.
- AC-20a: F1 and space space open the window, "/" lists commands with hints, Esc returns to READ, no-slash palette unchanged (q_01..06).
- AC-20b: /save-as (prompt, path saves, folder error kept), /reload (file re-read, no-file error), /term new/close (+ close without terminal error, bogus arg error), /term right|bottom "not available", /ws add (good, duplicate, bad dir, no arg), /ws save (new file, own file, refused after hand edit with file kept, other existing file refused), /ws recent, /ws remove (named, last root, bad name), /ws open (good, missing), /theme x4 + bad name (text kept), /hidden, /ignored, /help, /nope, bare "/" + Enter runs nothing.
- AC-20c: /run echo printed exactly once; busy terminal (/ai cat) -> second tab + notice, output once; /ai missing tool -> bash "command not found"; /q with unsaved -> Quit dialog and process still running; /q! exits, a.txt on disk unchanged ('alpha\n').
- 40x12: list, error with input kept, Esc -> READ (q_52..55).

Findings
1. LOW — app_workspace.rs:123: `/ws save <other existing file>` says "exists; it is replaced only when confirmed", but /ws save has no confirmation path (the notes list this as open), so the message promises something unreachable. Repro: /ws save ../third, then /ws save ../team (team exists) -> q_31.txt. Suggest wording "exists; /ws save does not replace it" or add the Replace prompt.

Known, not findings: D264/D265, D267 (/wrap unavailable), D271 (F1 inside a focused terminal goes to the program; the walk used ctrl+` to leave the terminal first).

Verdict T20: QC_PASS (one LOW finding).

## T20b
Verdict: QC_PASS (slash commands phase 2, after review round 1). Engine: pty (python, TERM=xterm-256color, legacy bytes, release binary, DISPLAY unset so OSC 52 clipboard); no browser (TUI).
- Full suite via run-heavy.sh: 26 result lines ok, 852 passed, exit 0 (qa-artifacts/T20b/qc/full-suite.txt). clippy --workspace --all-targets -D warnings exit 0; fmt --check exit 0; release build exit 0 (cargo found it fresh, 0.16 s: built by the developer's last run, fingerprints match the tree).
- docs table: `docs_table_matches_the_registry` ok; 36 command rows in docs/commands.md.
- Own walk qc/walk.py: 58/58 PASS (qc/walk.txt, screens qc/w_NN.txt). First run 54/58: 4 were walk errors (Esc after a notice leaves edit mode; /set notice is queued behind an older explorer notice "+1 more"; keymap template keeps `[section]` headers uncommented, only key lines commented), fixed in the walk, not the app.
- Covered: /rename (open tab follows, unsaved edit kept, save lands on new name, existing name refused, slash refused, no-file error), /copy-path and /copy-relpath (OSC 52 payload decoded: full path / `c.txt`), /recent (3 files, after rename d.txt replaces c.txt, bad name, 99, by name; recent-files.toml on exit), /sort /unique /upper /lower /trim (read-mode refusal, whole file, selection, one undo each), /indent 2|tab|4 on a 4-space C file with a doc comment (doc comment untouched) + bad arg, /eol lf|crlf (bytes on disk, already notice, bad arg), /lang rust + bad id, /set tab_width 2 + bad name/value/range/missing value (ctrl+h hidden toggle unchanged and still works), /config /keymap (created in temp XDG_CONFIG_HOME, key lines commented; existing files byte-identical), /keys.

## Rule check T20b
| Rule | Result | Evidence |
|---|---|---|
| AC-20b phase-2 commands, good and bad args, text kept on error | pass | walk 58/58 |
| AC-20d docs and /help match registry; snapshots | pass | docs_table test, 852 suite incl. snapshots |
| BASE-4 evidence | pass | qc/*.txt |
| BASE-14/15 no polling, only own processes | pass | background run, own pty sessions quit with /q! |
| BASE-17 probes leave no data | pass | temp HOME/XDG/state dirs only |
| BASE-25/38 fmt, clippy | pass | exit 0 |
| Known user questions D264/D265/D267/D271/D273 | not findings | per brief |

## T22 (QC, unified find ctrl+f) — QC_PASS
Engine: pty walk (python pty + VT model, TERM=xterm-256color, release binary rebuilt by QC), legacy bytes at 120x40, 80x24, 40x12, kitty bytes at 120x40; no browser. Artifacts: qa-artifacts/T22/qc/ (walk.py..walk4.py, w_*, x_*, y_*, t_* screens, run2_120.txt, run3.txt, full_suite.txt, release_build.txt).

| Check | Result |
|---|---|
| cargo test --workspace -j 4 via run-heavy (once) | 870 passed, 0 failed, 17 ignored, exit 0 |
| clippy --workspace --all-targets -D warnings; fmt --check | clean; ok |
| fresh release build (touch + run-heavy) | Finished release 25.3s |
| AC-22a File list, Enter jump (Ln 3 Col 16, READ), Esc box -> READ | pass at 3 sizes |
| AC-22b ctrl+f again -> Project, grouped by file; two roots -> root header rows (qcw2.., qcr2..); Enter opens file at match (ro.txt Ln1) | pass |
| AC-22c `foo/i`, `/fo+/`, `/fo+/i`, `a\/b` (finds only `a/b`), `/fo(/` -> "invalid regex ... unclosed group", no search | pass |
| AC-22d f3 next (2/4 -> 3/4); shift+f3 kitty (`CSI 13;2~`) back; legacy shift+f3 unchanged = D264 (known); shift+Enter kitty previous (3/3, 2/3) | pass |
| ctrl+r File: Enter one (1/3 -> 1/2), ctrl+a all ("Replaced 2 occurrences"), Esc -> READ; ctrl+s writes `PIN 1\nPIN 2\nPIN 3\n` on disk | pass |
| ctrl+r on non-UTF-8 (read-only) file: stays READ [read-only], bytes on disk unchanged | pass |
| ctrl+r Project: preview [x] per file, Space unchecks r2, confirm dialog "2 matches in 2 files" (default Cancel; left+Enter = Replace); disk: r3 `PIN 5`, r2 unchanged, ro.txt (mode 0444) "skipped: file is read-only" unchanged | pass |
| 1M-line file: open 2.2s; each key (type, down, enter, f3, switch) answered within the 50 ms poll floor; 10 matches listed | pass |
| terminal open, short windows 80x12, 40x10, 120x14: Find and Replace rows keep their rows, list/editor shrink | pass |
| docs/keymap.md "The find box", README find section match observed behaviour | pass |

Observations (not defects): (1) after Enter jumps, focus is the editor and Esc there leaves the box open (Esc in the box closes it); matches spec wording. (2) A 0444 file opens editable in the editor (read_only means non-UTF-8 only); file-scope replace edits its buffer; project replace skips it safely. Pre-existing. (3) 40x10 + terminal: replace row drawn, hint "enter one - ctrl+a all" dropped for width.

## Rule check T22
| Rule | Status | Evidence |
|---|---|---|
| AC-22a..e | pass | table above; snapshots covered by suite (80x24, 120x40, 40x12) |
| BASE-4/5 | pass | full suite, clippy, fmt, release build outputs in qa-artifacts/T22/qc |
| BASE-14/15/17 | pass | no polling loops of heavy runs; only own pty sessions killed; temp dirs only |
| BASE-19 | pass | results written as checked |
| USER-5 | pass | legacy ctrl+f/ctrl+r bytes walked |
| COMPACT | pass | short windows keep box rows |

## T23 (explorer right-click menu, D262/D276)
Engine: release binary driven through python pty (TERM=xterm-256color, SGR mouse bytes), 120x40, 80x24, 40x12. Artifacts: qa-artifacts/T23/qc/ (walk.py, edge.py, *_out.txt, w_NN.txt screens).

| Check | Result |
|---|---|
| fmt --check, clippy --workspace --all-targets -D warnings | pass (fmt.txt, clippy.txt) |
| fresh release build (touched main.rs, run-heavy) | pass, 1m58s (release.txt) |
| full suite once (run-heavy, load avg 12-18) | 1 failure: vr-syntax `every_sample_equals_a_fresh_parse_after_each_random_char_edit` (xml/vue incremental vs fresh spans). vr-syntax has no T23 change (git clean); the dev's full run passed it (883/0). Rerun alone once under load: failed again. Not attributable to T23 (outside the diff). All vr-ide tests ok (481 passed; the recent_files flake did not occur) |
| walk (3 sizes, 18 checks each, disk verified): right click file opens menu; Esc and outside click close; Rename via mouse (disk rename); New... File/Folder choice (file and folder created in zdir); copy+paste file and folder (recursive, deep.txt copied); cut+paste moves (source gone); blank-area right click -> menu, dimmed Rename click does nothing (menu stays), Paste at root works; keyboard End+Enter -> Delete asks, Move to Trash -> file gone, present in $XDG_DATA_HOME/Trash/files; top row menu | pass 18/18 per size (walk_out.txt) |
| edge: right click at bottom-right / right edge / bottom-left of explorer at all 3 sizes: box fully drawn (4 corners) | pass |
| mouse-grabbing program in terminal (`?1000h ?1006h; cat -v`): program receives `^[[<2;6;13M/m`, no menu | pass x3 sizes (edge2_out.txt) |
| docs (README explorer section, docs/keymap.md row) vs behaviour | match |

Notes: the first walk's "menu near bottom-right" FAIL x3 was my script clicking in the editor area (outside the explorer, no menu by design); rerun inside the explorer = pass. The explorer is at most ~29 cols wide, so the screen-right clamp is not reachable by mouse at these sizes; covered by dev unit tests only.

## Rule check T23
| Rule | Status | Evidence |
|---|---|---|
| D262 request (rename, new file/folder, delete to trash, copy, cut, paste; files+folders) | pass | walk disk checks |
| D276 design calls (order, dimming, close rules, placement) | pass | walk + snapshots |
| BASE-4/5 | pass | qa-artifacts/T23/qc outputs |
| BASE-14/15/17 | pass | no polling loops of heavy runs; only own pty sessions; temp dirs removed; trash redirected to a temp XDG_DATA_HOME |
| BASE-19 | pass | written as checked |
| Full suite | note | unrelated vr-syntax failure under load, see above |

## T19 — terminal panel on the right (D227, D275, D277)

Verdict: QC_PASS. Engine: real release binary in a python pty (TERM=xterm-256color, legacy bytes ESC[18;5~ / ESC[19;5~ / ESC[20;5~, SGR 1006 mouse), scripts qa-artifacts/T19/qc/qc_walk.py and qc_walk2.py, logs walk.log, walk2.log, screens walk_*.txt.

Guards (logs in qa-artifacts/T19/qc): cargo fmt --check clean; cargo test -p vr-ide -p vr-config: vr-config 16+22+8, vr-ide 496 passed / 0 failed / 7 ignored (no timing failure this run); clippy --workspace --all-targets -D warnings: no warnings; fresh release build via run-heavy.sh finished (3m33s).

PTY walk (`stty size` rows x cols inside the terminal; "col" = editor column = screen minus explorer):
| Size | Explorer | bottom | ctrl+f7 right | ctrl+f8 grow | ctrl+f9 x2 | ctrl+f7 back |
|---|---|---|---|---|---|---|
| 120x40 | off | 15x120 | 36x48 (sep 71) | 37x54 | 37x42 | 15x120 |
| 120x40 | on | 15x90 | 36x36 (sep 83) | 37x41 | 37x32 | 15x90 |
| 80x24 | off | 9x80 | 19x32 | 21x36 | 21x28 | 9x80 |
| 80x24 | on (col 60) | 9x60 | 19x24 | 21x27 | 21x21 | 9x60 |
| 60x20 | off | 7x60 | 15x24 | 17x27 | 17x21 | 7x60 |
| 60x20 | on (col 44) | 7x44 | stays bottom 7x44 | 8x44 | 6x44 | bottom |
| 59x20 | off/on | 7x59 / 7x43 | stays bottom | 8 rows | 6 rows | bottom |
| 40x12 | off/on | 4x40 / 4x24 | stays bottom | 5 rows | 4 rows | bottom |
Shrink after grow changes the size at once in both modes (grow, shrink, shrink gives less than the start).
Other flows (walk2.log): in-grid click with a program in mouse mode (1000+1006) reaches it (`^[[<0;19;9M`); separator drag 71 -> 55 moves the line, PTY 37x64 (no drag byte reaches the program); /term right, /term right again ("already on the right", notice queue shows "+1 more"), /term bottom; /set terminal_position right applies at once; /set terminal_position diagonal -> "unknown variant `diagonal`, expected `bottom` or `right`", state unchanged; /set terminal_width_percent 95 -> "must be between 10 and 90"; 50 -> separator 59 (60 columns); settings.toml `terminal_position = "right"` starts on the right (sep 71); find box (ctrl+f, "foo") at 80x24 right keeps its rows left of the separator (walk_find_right.txt). The first scripted "FAIL" lines in walk2.log are my checker picking the editor welcome box's border; the screens (walk_slash_*.txt, walk_set_bad*.txt) show the expected state.
Docs: docs/commands.md, docs/keymap.md, README.md, defaults/settings.toml comments match behaviour (key, 60-column rule, 10-90, session-only).

## Rule check T19
| Rule | Result | Evidence |
|---|---|---|
| BASE-10 tests + guards | pass | test/clippy/fmt/release logs above |
| BASE-14 no sleep/poll | pass | background runs, notified (one polling loop of mine was stopped with TaskStop) |
| BASE-15 own processes only | pass | walk kills its sessions; killed only my own walk pids |
| BASE-17 no data left | pass | temp dirs only |
| BASE-18 no own agents | pass | none |
| BASE-19 results written as checked | pass | this section, logs |
| COST heavy via run-heavy | pass | test, clippy, release via run-heavy.sh |
| Scope | pass | nothing committed, stashed or restored; FR-13 files untouched |

## W-2
Engine: own pty walks (python pty, TERM=xterm-256color, legacy bytes, SGR mouse) on the release binary; scripts and screens in qa-artifacts/W-2/qc/ (walk1/2/3.py, probe*.py, w1_*, w2_*, w4_* screens, logs).
Guards: fmt --check rc 0; clippy --workspace --all-targets -D warnings rc 0; full `cargo test --workspace` rc 0 (vr-ide 518 passed, 0 failed, no flaky), release build rc 0 (binary newer than all sources).
| Criterion | Evidence | Result |
|---|---|---|
| AC-W1 `vr-ide a b`: two root nodes, open from both, status bar | w1_02..04: `a/alpha.txt`, `b/beta.txt` in status bar | pass |
| AC-W1 ctrl+p both roots | w1_05: rows `mv.txt  b`, `alpha.txt  a`, `deep.txt  a/sub` (name + dim root/dir, not literal `root/rel`) | pass (note) |
| AC-W2 UI: project search grouped by root | probe: groups a / b / c, then file | pass |
| AC-W3 /ws add live (explorer, index, search) | w1_09..11 (Go to file and project search find c after add; the first search check in the walk was a timing slip in my script, confirmed in a separate probe) | pass |
| AC-W3 Remove Folder keeps files and tabs; edit+save in tab of removed root | w4_01/02: file on disk "beta\nZ" | pass |
| AC-W3 missing root | w2: `c (missing)` row, welcome "missing", removable; one startup notice | pass |
| Edge: nested / same twice / symlinked root | ws file with b twice, a/inner, lnk->real, real: one b, inner refused with notice, lnk/real shown once; /ws add dup ignored, nested refused | pass |
| AC-W4 /ws save + /ws open | file `roots = ["a","b","c"]`; fresh instance with `a` + /ws open shows a b c | pass |
| AC-W4 welcome WORKSPACES + click | list shows recent sets (current set hidden, numbers keep gaps); click on a row opened a b c | pass |
| AC-W5 cut/paste across roots refused, incl. via folder link; copy works; rename confined | w1_07/08, w4: files unchanged on disk; /rename ../b/x refused | pass |
| AC-W6 terminal in active file root | pwd = .../a (key), dev walk covers palette root pick and web | pass |
Findings (all minor/cosmetic, none blocking):
- minor: welcome PROJECT prints "1 files" (singular not handled), crates/vr-ide/src/ui/welcome.rs (repro: root with one file).
- minor: with 3+ roots on a short window the PROJECT block collapses to "+N more" (by design for height); informational.
- info: Go to file shows `name  root/dir` rather than the spec's `root/relative/path` string; same information.
## Rule check W-2
| Rule | Status | Evidence |
|---|---|---|
| BASE-4/5 | pass | fmt, clippy -D warnings, full suite, release rc 0 |
| BASE-10 | pass | one full suite run |
| BASE-14/15 | pass | background chain + Monitor, no polling, only own processes |
| BASE-17 | pass | temp dirs under /tmp, private config/state dirs, nothing left running |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | written incrementally to this section |
| BASE-24 | pass | no commit/stash/checkout; only files under .ai-dev-team/workspace written |
| COST | pass | heavy via run-heavy.sh |

## T24
Engine: own pty walks (python pty, TERM=xterm-256color, legacy bytes + SGR mouse, temp config/state/HOME) on the release binary; script and screens in qa-artifacts/T24/qc/ (walk.py, qc_*.txt, reload_result.txt, keys_term_result.txt).
Guards: fmt --check rc 0; clippy --workspace --all-targets -D warnings rc 0; release build rc 0. Full suite: first run stopped at vr-core `watch_debounces_burst` (timing under load, untouched crate; passes alone twice) so the suite was run again with --no-fail-fast: 939 passed, 0 failed (full2.log). Flaky noted, not hidden.
| Criterion | Evidence | Result |
|---|---|---|
| f4 / shift+f4 (ESC[1;2S) next/prev; f3/shift+f3 nothing | K1-K5 | pass |
| Replace dialog One/All/Cancel, One default, Esc, undo after All is one step | K6-K12 (K10/K11 script asserts were wrong; screens show "X X X", "Replaced 2 occurrences", one ctrl+z back to pre-All text) | pass |
| /wrap, /term new, /term close unknown; palette without New/Close Terminal; ctrl+f12/ctrl+f4 nothing | K13-K16 | pass |
| F1 from focused terminal, Esc back | covered by dev walk 31/31 and unit test; not re-walked by me | pass (indirect) |
| ctrl+` from editor / explorer / find box / terminal, first terminal created | T[...] at 120x40, 80x24, 40x12, right 120x40, 80x24: 65/65 | pass |
| '+' opens terminal in active file's root (rootB, multi-root), 'x' closes tab, last 'x' hides | same 65/65 | pass |
| Live reload: external settings (theme, tab_width), broken file keeps old + error, unknown theme refused, /config save, keymap.toml external, symlinked settings.toml with real file open (external, ctrl+s, after tab close) | reload_result.txt 13/13 | pass |
| Welcome "1 file" | W1 | pass |
Findings: none blocking. Open D295 (terminal 'x' closes running program without asking) is known.
## Rule check T24
| Rule | Status | Evidence |
|---|---|---|
| BASE-4/5 | pass | fmt, clippy, release rc 0; suite 939/0 (one flaky vr-core timing test, passes alone) |
| BASE-10 | pass | full suite run once plus a --no-fail-fast rerun after the first stopped early |
| BASE-14/15 | pass | run-heavy, background + monitor, own walk processes killed; foreign pid 1456662 untouched |
| BASE-17 | pass | temp config/state/HOME dirs only |
| BASE-18/19/24 | pass | no sub-agents, written incrementally, no commit/stash/checkout |
