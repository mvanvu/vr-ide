
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
