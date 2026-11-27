# Review T9 — vr-search engine

**Verdict: CHANGES T9** (2 major, 0 blocker)

Commands: `cargo test -p vr-search` → `21 passed; 0 failed; 1 ignored`. `cargo clippy -p vr-search --all-targets --no-deps -- -D warnings` → clean.
Probe: `qa-artifacts/T9-review/probe` (path dependency on vr-search, its own temp dir).

## Findings

[major] crates/vr-search/src/replace.rs:183 + :206-212 — changed-since-preview check runs before the temp write and fsync, so the overwrite window is far wider than the comment says (BASE-32, D25)
Scenario: `apply_one` compares the bytes at :183, then `write_atomic` creates the temp file, writes the whole file and runs `sync_all()` (tens to hundreds of ms on a slow disk or a large file), then renames. An AI agent that saves the file during that write/fsync is silently overwritten by the rename: its change is lost and no error comes back. The comment at :180-182 says the window is "microseconds"; it is not.
Fix: write and `sync_all` the temp file first, then re-read and compare immediately before `fs::rename`; on a mismatch remove the temp file and return the "changed on disk" error. Correct the comment and log the remaining window (re-read + rename) in decisions.md like D32. Closing it fully on Linux needs `renameat2(RENAME_EXCHANGE)` (swap, check the swapped-out bytes, swap back on mismatch) and so `rustix` in vr-search's manifest: a leader decision (BASE-8). Test: hard to race deterministically; at least a unit test of a split helper (e.g. `write_temp` then `commit_if_unchanged`) that changes the file between the two steps and asserts the agent's bytes survive.

[major] crates/vr-search/src/project.rs:169 — project search reads every file whole into memory, with no size limit (Review Focus 4)
Scenario: a project with an un-ignored 3 GB `data/dump.sql` or model file → each walker thread `fs::read`s the whole file before the NUL check and before any cancel check; several such files across the parallel threads allocate several GB, and an allocation failure aborts the process (no panic hook, so modified buffers are not written to recovery, C6). Cancel does not help while a read is in progress. The same holds for `plan_replace` (replace.rs:81) which also keeps `original` and `replaced` copies of every previewed file.
Fix: check `entry.metadata().len()` and skip (count and report) files above a cap, or search with `searcher.search_path` (streaming reads, `BinaryDetection::quit(b'\0')`) and read a file whole only for the lone-CR exact path under a size cap. Test: a fixture above a lowered cap (make the cap a parameter of `FileSearch`) is skipped and Done still arrives.

[major] crates/vr-search/src/project.rs:254-265 — prefilter misses matches for patterns with an inline `(?-m)` flag (D33 says every match is re-checked; a candidate the prefilter rejects never is)
Scenario (probe): file `x\nfoo\nfoo bar\n`, regex `foo(?-m)$` → in-buffer `find_all` finds 1 match, `search_project` finds 0. The prefilter forces `multi_line(true)`, but an inline `(?-m)` turns `$` back into end-of-buffer, so line 2 is never a candidate. The search silently skips a real match; replace previews then disagree with the results list.
Fix: return `None` from `prefilter` (exact split) when the regex pattern contains an inline flag group that sets or clears `m` (simplest: any `(?`), like the existing `\A`/`\z` guard. Test: the probe case in `project_search_line_numbers_follow_d31`.

[minor] crates/vr-search/src/find.rs:80-81 — `$1_old` expands as a group named `1_old` (empty)
Scenario (probe): `fn (\w+)` → `fn $1_old` yields `fn ` (capture dropped). This is the regex crate's rule, but users coming from VS Code (D10) expect `fn alpha_old`; the preview shows it, so it is visible, not silent. Fix: either parse `$<digits>` greedily as a number (pre-process the replacement to `${n}`) or document in D36 so T10 shows a hint.

[minor] crates/vr-search/src/find.rs:92-101 — whole-word check differs from `\b` on Unicode marks and on alternations
Scenario (probe): NFD `vie\u{301}t` with whole-word `vie` → matches (U+0301 is not `is_alphanumeric`, while `\w` includes marks). Regex `ab|abc` whole-word on `abc` → no match (leftmost-first picks `ab`, rejected, retry starts one char later). Fix: treat `General_Category=Mark` (and connector punctuation) as word chars, or build whole-word as `(?:\b(?:pat)\b)`-style wrapping with the retry kept only for empty/odd cases.

[minor] crates/vr-search/src/replace.rs:191-218 — atomic write details
- :201-205 temp name = `.{name}.vr-search-PID-SEQ.tmp`: a name over ~225 bytes exceeds NAME_MAX → ENAMETOOLONG error for that file (safe, but refused). On `create_new` failure (:207) the cleanup at :215 removes the pre-existing file of that name.
- No directory fsync after rename; a crash may roll the rename back (old content, not corruption).
- Rename breaks hard links and replaces a read-only (0444) file through directory write permission, unlike an editor save to a read-only file.
- :179 apply re-canonicalizes without re-checking the project root (a symlink retargeted outside the root between plan and apply is followed; needs identical bytes, so only noted).
- BASE-16: duplicates T4's atomic save (vr-core, uncommitted at the time, so not a fail); once T4 lands, use one helper so the temp names are also recognised by `vr_core::is_save_temp` / the watcher and the explorer does not flash the temp file.

[minor] project.rs:165 + replace.rs — a symlinked file inside the root is listed twice (link and target); confirming both in project replace reports the second as "changed on disk" (safe, misleading message). Fix: dedupe confirmed paths by canonical path in `apply_replace` and report "same file as …".

Checked and fine: char offsets via `byte_to_char` on the same string; zero-length advance by `next_char` over multi-byte chars (probe `tiếng`); `$0`/`${name}`/`$$`; CRLF/BOM/lone-CR/no final newline preserved; NUL and non-UTF-8 refused; `..` and absolute paths outside the root refused at plan; temp file `create_new`, removed on error, mode set before content; symlink written through; 100k cap with exact truncation and Error before Done; Done always last (walker panic is joined and ignored); long-line window on char boundaries; regex DoS bounded by the regex crate (linear time, default 10 MB compiled-size limit → `InvalidRegex`).

## Files read
- crates/vr-search/Cargo.toml — read (unchanged)
- crates/vr-search/src/lib.rs — read
- crates/vr-search/src/find.rs — read
- crates/vr-search/src/project.rs — read
- crates/vr-search/src/replace.rs — read

## Rule check T9
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | `git status --short crates/vr-search` → lib.rs modified, find/project/replace.rs new; nothing else |
| BASE-2 | pass | plan Contracts vr-search match; additions (Hunk, private bytes, `replacements()`, refs for root/q, non-terminal Error) are additive and logged D33–D37 |
| BASE-3 | fail (minor) | most post-hoc tests have recorded mutation reds; `find_regex_multiline_off`, `replace_all_zero_length_no_loop`, `replace_all_unicode_offsets` were never red against missing code (only wrong expectations). Fix: record a mutation red for each (e.g. drop the `last_end` guard, use byte offsets) |
| BASE-4 | pass | notes carry test/clippy/fmt output; reproduced 21 passed, clippy clean |
| BASE-6 | pass | English, why-comments; the "microseconds" claim at replace.rs:180 is wrong (covered by the first major) |
| BASE-8 | pass | Cargo.toml unchanged; all deps from the workspace manifest |
| BASE-16 | pass | Reuse table present; write_atomic duplication noted as follow-up after T4 lands |
| BASE-32 | fail (major) | D15/D31/D33–D36 honoured; D25 "never overwrite unseen changes" undermined by the check-before-fsync window (first major) |
| AC-9 | pass | `project_search_respects_gitignore` (non-repo, nested .gitignore) |
| AC-10 | pass | `replace_all_regex_captures`: one Transaction, one undo, second undo false |
| AC-11 | pass | streaming tests + release probe (first batch 1.3–60 ms on 10k–81k files); col exact for Enter |
| AC-12 | pass (library) | preview without write, only confirmed written, changed file refused; open-buffer undo is T10 |
| RF-4 | fail | whole-file reads without size cap (second major) |
| RF-5 | pass | zero-length / captures, one undo, no loop |

## Round 1 re-check

**Verdict: CHANGES T9 R1**: 1 major (a rare data-loss path in the new swap-back code), 2 minors. All three round-0 majors and the round-0 minors are fixed.

Scope read: `git diff HEAD -- crates/vr-search Cargo.lock` (vr-search/Cargo.toml +rustix per D45; Cargo.lock +1 line), untracked find.rs / project.rs / replace.rs in full (code and new tests). The root Cargo.toml diff (ropey `cr_lines`) is not T9's.
`cargo test -p vr-search` → `31 passed; 0 failed; 1 ignored`. Probe (`qa-artifacts/T9-review/probe`): `$1_old` → `fn alpha_old`, `$$1` → `$1`, `${1}x`, `$0`; whole-word `ab|abc` finds `abc`; NFD `vie\u{301}t` is no longer a whole-word hit for `vie`.

### New finding

[major] crates/vr-search/src/replace.rs:368-370 with :392 and :395-401 — after the exchange, `tmp` can hold the other writer's version, and it is still deleted
Scenario A: the exchange succeeds, the swapped-out file differs (an agent wrote it), and the swap-back `exchange_files` fails (EIO, ENOMEM, or the temp name was touched). The error says "the other version is at <tmp>", but `commit_replace` then runs `if tmp.exists() { remove_file(tmp) }`, so the agent's version is deleted and the message points to a file that no longer exists.
Scenario B: after the exchange, `holds(tmp, expected)?` fails (open hits EMFILE/ENFILE under fd pressure, or a read error). The `?` returns early, the target already holds the replacement, the swapped-out file (possibly the agent's version) is deleted the same way, and the user gets an I/O error for a file that was in fact changed.
Both paths are rare, but the outcome is silent loss of the very write this code protects. The normal swap-back path is safe: the agent's version goes back to the target, and only our own replacement bytes are deleted.
Fix: have `commit_inner` say whether `tmp` holds bytes we own (our replacement, or the verified original). Delete `tmp` only in those cases. On a failed swap-back, or a failed read after the exchange, keep the file and rename it to a visible name that `is_save_temp` does not match (e.g. `<name>.vr-ide-conflict-PID-N`), so the explorer shows it and the error names the real path. Test: make the second exchange fail through the existing seam (e.g. a `swap: &mut dyn FnMut(&Path,&Path)->io::Result<()>` that fails on its 2nd call) and assert the agent's bytes still exist somewhere named in the error.

### Minors
[minor] crates/vr-search/src/find.rs:39: whole word with an extended-mode pattern that ends in a comment fails to compile. Probe: regex `(?x)foo # c` works alone but gives `InvalidRegex: unclosed group` with whole word on, because the comment swallows the wrapper's `)`. Fix: wrap as `(?:{source}\n)` when the pattern turns on `x` (a newline ends an `x`-mode comment), or document that combination as unsupported.
[minor] Remaining window after the exchange (documented in the `commit_replace` comment): two agent writes within microseconds can still lose the later one. Example: the agent renames file B over the target after our swap, so a swap-back moves B into `tmp`. This belongs in the D? line the developer proposed; nothing to change in code.

### Round-0 findings
| Finding | Status | Evidence |
|---|---|---|
| M1 check before fsync | fixed | `write_temp` writes and fsyncs first. `commit_inner` then checks again → `renameat2(EXCHANGE)` → compares the swapped-out bytes → swaps back on a mismatch. The fallback (EINVAL/ENOSYS/non-Linux) checks again right before `rename`. Dir fsync runs on success. Test `commit_keeps_agent_write_made_before_the_swap` covers both branches, with mutation reds. The swap-back failure is the new major above |
| M2 unbounded reads | fixed | `read_capped` allocates at most cap+1 bytes even if the file grows; search counts oversized files and sends one `Error` before `Done`; plan refuses them. Previews keep `(len, SipHash)` and apply re-reads (capped at the previewed length) and recomputes. Memory ≤ threads × 32 MB. SipHash uses fixed keys, which is fine here: the final commit compares full bytes, not the hash |
| M3 prefilter `(?-m)` | fixed | `prefilter_safe` sends any `(?` other than `(?:`, `(?<` and `(?P` to the exact path; the parity test covers 5 flag cases |
| `$1_old` | fixed | `numbered_groups_braced`: `$<digits>` is the numbered group, `$$` is kept, and `${..}` / `$name` are unchanged (probe above). This amends D36, so the leader should number the line |
| whole word vs `\b` | fixed | `\b{start-half}(?:pat)\b{end-half}`. The raw pattern is compiled first, so `a)(b` stays invalid, and group numbers are unchanged (`(?:`). One gap remains (`(?x)` minor) |
| temp names / NAME_MAX / cleanup | fixed | `.vr-ide-save-PID-sN.tmp` uses T4's prefix and suffix, so `is_save_temp` matches it, and the `-sN` suffix cannot collide with T4's `-N`. AlreadyExists → next name, and a file it did not create is never deleted. 250-char name test passes |
| read-only / hard links | fixed | read-only and nlink > 1 refused with an error item (D41 equivalent) |
| root re-check at apply | fixed | `target.starts_with(p.root)`; retarget test |
| link listed twice | fixed | search skips a link whose target is inside the root (documented gap when that target is gitignored); apply dedupes by canonical path with "same file as …" |
| BASE-3 old tests | fixed | mutation reds recorded for the three tests |

### Hard links: replace refuses, T4 save splits (D46)
This inconsistency does not matter. Project replace is a batch with no per-file prompt, so refusing with a visible error item is the safer default. Save is an explicit single-file action where the D46 notice can be read. Two follow-ups to log, neither a T9 code change:
- The error text should tell the user how to proceed, e.g. "open the file and save it to replace it there". T10 can also route hard-linked files to the buffer path, as it does for open files, so the replace becomes undoable and goes through D46.
- Record the difference in decisions.md next to D46.

### Rule check T9 (round 1)
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only vr-search files + its Cargo.toml + 1 Cargo.lock line |
| BASE-2 | pass | public signatures unchanged; `FilePreview` lost its derived `PartialEq` and got a manual one over the public fields (additive in effect); `Limits`/`*_with` are crate-private |
| BASE-3 | pass | red line per new test; mutation reds for tests that passed on their first run |
| BASE-4 | pass | notes evidence; reproduced 31 passed |
| BASE-6 | pass | comments say why; the race window is stated correctly |
| BASE-8 | pass | rustix per D45 (workspace dependency) |
| BASE-16 | pass | temp scheme aligned with T4; merging the write helpers after T4 lands is logged |
| BASE-32 | fail (major) | D25: data-loss path on swap-back failure (above). D15/D31/D33–D36/D41 honoured |
| AC-9 / AC-10 / AC-11 / AC-12 | pass | unchanged + parity/limit tests |
| RF-4 | pass | 32 MB cap, oversized files reported |
| RF-5 | pass | unchanged |

## Round 2 re-check

**Verdict: APPROVE T9 R2.** The R1 major and both minors are fixed. `cargo test -p vr-search` → `34 passed; 0 failed; 1 ignored`.

| Item | Status | Evidence |
|---|---|---|
| R1 major: swapped-out file deleted | fixed | `commit_inner` (replace.rs:407-456) returns `TmpHolds`. `Ours` is returned only when `tmp` holds our replacement: precheck failed, swap-back succeeded, a non-exchange error, or the fallback rename. `Ours` is also returned for the verified original (`holds(tmp)` true) or for nothing (rename done). `Unknown` is returned exactly after a successful exchange whose verify read fails, or after a failed swap-back. `commit_replace` deletes `tmp` only for `Ours` (:375-383). For `Unknown` it calls `keep_conflict` and names the path in the error (:384-394). Tests `commit_keeps_other_version_when_swap_back_fails` and `..._when_verify_fails` were red first and are now green; the mutation "delete instead of keep" turns them red. |
| `keep_conflict` | ok | `hard_link` + unlink never replaces an existing name: AlreadyExists → next N, at most 100 tries. The name is cut to ≤ 200 bytes on a char boundary. The visible `<name>.vr-ide-conflict-PID-N` does not match `is_save_temp` (asserted). |
| `keep_conflict` failure | safe | Any `hard_link` error other than AlreadyExists → `break` → `tmp` is left untouched and its own path goes into the error, so nothing is deleted. This path is barely reachable. `Unknown` only follows a successful `RENAME_EXCHANGE`, and every filesystem that supports it (ext4, xfs, btrfs, tmpfs, f2fs) also supports hard links. A read-only directory is impossible at that point, because the temp file was just created and swapped there. That leaves ENOSPC/EDQUOT on the directory entry, or EMLINK. In those cases the other version stays at the hidden `.vr-ide-save-*.tmp` path: the explorer shows it only with dotfiles on, but no code in vr-core, vr-fs or vr-ide deletes stale save temps, so it is not lost. Optional improvement, not required: fall back to `renameat2(RENAME_NOREPLACE)` before giving up. |
| R1 minor: `(?x)` + whole word | fixed | find.rs:39-46: the wrapped build is retried with `\n` before `)` only when the wrapped build fails after the raw pattern already compiled. In `x` mode the newline only ends the comment. In non-`x` mode the first build does not fail, so the retry is never reached and the meaning cannot change. The error from the first wrap is kept if the retry also fails (e.g. size limit). Test `whole_word_with_extended_mode_comment`. |
| R1 minor: two-write window | documented | the comment on `commit_replace` and the proposed D? line state it |
| Hard-link message | fixed | error says "open the file and save it" (D48), asserted |

Rule check (round 2): BASE-3 pass (red per new test plus mutation), BASE-6 pass, BASE-32 **pass** (D25: the other writer's version is never deleted), others unchanged from round 1. The leader still needs to number the five `D? (T9)` lines in the notes.
