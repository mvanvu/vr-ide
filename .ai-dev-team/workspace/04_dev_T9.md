# T9 vr-search engine: find/replace, project search, project replace
Task base: c421310273572eeadb4d5bf5b3fbb39da9120e06 (HEAD fc0e80c). Owner be (library). Files: crates/vr-search/src/{lib,find,project,replace}.rs.

## Progress
- read: brief-common, plan (Contracts vr-search, Task T9, Review Focus 4/5), spec AC-9..AC-12, D15/D24/D25/D31, capabilities, vr-core lib/transaction/text, vr-fs file_index walk config.
- skeleton: lib.rs (Query, QueryError, re-exports), stub find/project/replace with `todo!()` so tests compile and fail.
- red `find_plain_case_insensitive`: panicked at find.rs:10 (todo!) → green after Matcher + line_spans + find_all.
- red `find_whole_word`: left [(0,3),(7,10),(11,14),(17,20),..] vs right [(0,3),(17,20),..] → green after hand-rolled each_match with whole-word retry.
- `find_regex_multiline_off`: passed on first run (line_spans with D31 breaks was written in the first green step); `invalid_regex_is_error`: red (todo! in replace_all_tx).
- red `replace_all_regex_captures`: panicked (todo!) → green after replace_all_tx + Matcher::expand; one undo verified with vr_core::Buffer.
- `replace_all_zero_length_no_loop` / `replace_all_unicode_offsets`: first run failed on two wrong expectations in the tests themselves (empty regex is "match nothing" by design; offsets miscounted by me) — fixed the expectations; zero-length loop guard was written in the whole-word step.
- red `project_search_respects_gitignore`: panicked at project.rs:29 (todo!) → first impl run: left [] (grep config error "mismatched line terminators, matcher CRLF vs searcher Byte(10)") → searcher set to LineTerminator::crlf(), searcher errors fall back to the exact line split → green.
- project tests written after project.rs was implemented (streams, timer flush, cancel, binary, D31 lines, long-line window, invalid regex/bad root) passed on the first run. Red proof by mutation instead (each mutation applied to a copy of project.rs, test run, file restored with cp):
  - MAX_BATCH 200→200_000: `project_search_streams` FAILED (project.rs:450)
  - drop NUL check: `project_search_skips_binary` FAILED left ["blob.bin","text.txt"]
  - grep path even with lone \r: `project_search_line_numbers_follow_d31` FAILED left ("cr.txt",1,3,...,"a\rneedle\rb")
  - drop cancel check in walk: `project_search_cancel_stops` FAILED (project.rs:508)
  - require_git(true): first FAILED to fail (fixture had a .git dir, so it was a repo) → fixture split (non-repo part first, .git added after) → FAILED left [... "run.log", "target/debug/x.rs"]
  - drop .git filter: FAILED left [".env", ".git/config", ...]; hidden(true): FAILED left ["src/a.rs","src/sub/keep.rs"]
  - no flush on timeout: `forward_batches_flushes_on_timer_while_walk_runs` FAILED (project.rs:479)
  - window from line start: `project_search_long_line_is_windowed` FAILED left "" right "needle"
- cancel test first version raced (asserted no Batch after cancel while batches were already queued) → rewritten: pre-set cancel = Done{files:0} and no Batch; mid-run cancel = files < 3000.
- red `plan_replace_previews_hunks`: error[E0599] no method `replacements` on FilePreview (stub) → green after plan_replace/preview_bytes.
- red `apply_replace_only_confirmed_files`: panicked "not yet implemented" (replace.rs:141) → green after apply_replace/apply_one/write_atomic.
- replace tests written after apply_replace (keeps_crlf, skips_file_changed_since_preview, refuses_unsafe_files, keeps_mode_and_symlink) passed first run; mutation proof (copy, mutate, run, restore with cp):
  - drop BOM re-add: keeps_crlf FAILED (left lacks 239,187,191); CRLF→LF between lines: keeps_crlf FAILED (left …119,10…)
  - drop on-disk re-check: skips_file_changed FAILED (replace.rs:312); drop root containment: refuses_unsafe FAILED (replace.rs:330)
  - drop set_permissions: keeps_mode FAILED left 436 (0o664) right 488 (0o750); write to link path instead of target: FAILED (symlink replaced)
  - preview lookup ignores path: only_confirmed FAILED (replace.rs:262)
- ignored probe `project_search_first_batch_latency` added; release measurements below.
- clippy: `type_complexity` in a test tuple → type alias; fmt on own files; temp file permissions now set before content is written (self-review).
- decisions.md: 5 `D? (T9)` lines; capabilities.md: 3 vr-search lines.

## Summary
- Files changed: crates/vr-search/src/lib.rs, crates/vr-search/src/find.rs (new), crates/vr-search/src/project.rs (new), crates/vr-search/src/replace.rs (new). Team files: this notes file, workspace/decisions.md (appended), capabilities.md (vr-search section).
- Tests added (21 + 1 ignored): find:: find_plain_case_insensitive, find_whole_word, find_regex_multiline_off, invalid_regex_is_error, replace_all_regex_captures (AC-10), replace_all_zero_length_no_loop, replace_all_unicode_offsets · project:: project_search_respects_gitignore (AC-9, D15), project_search_streams (AC-11, RF-4), forward_batches_flushes_on_timer_while_walk_runs, project_search_cancel_stops, project_search_skips_binary, project_search_line_numbers_follow_d31, project_search_long_line_is_windowed, project_search_reports_invalid_regex_and_bad_root, project_search_first_batch_latency (#[ignore], env-driven probe) · replace:: plan_replace_previews_hunks, apply_replace_only_confirmed_files (AC-12), apply_replace_keeps_crlf (+BOM, lone CR, no final newline), apply_replace_skips_file_changed_since_preview, plan_replace_refuses_unsafe_files, apply_replace_keeps_mode_and_symlink.

## Commands run + result (trimmed)
```
$ cargo test -p vr-search
test find::tests::replace_all_regex_captures ... ok
test find::tests::replace_all_zero_length_no_loop ... ok
test project::tests::project_search_respects_gitignore ... ok
test project::tests::project_search_streams ... ok
test project::tests::project_search_cancel_stops ... ok
test replace::tests::apply_replace_only_confirmed_files ... ok
test replace::tests::apply_replace_keeps_crlf ... ok
... (21 lines ok, 1 ignored)
test result: ok. 21 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.10s
$ cargo clippy -p vr-search --all-targets --no-deps -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.36s
$ cargo fmt -p vr-search --check   -> clean
$ cargo build -p vr-search 2>&1 | grep -c "unused dependency"   -> 0 (all manifest deps used: regex, ropey, vr-core, ignore, grep-searcher, grep-regex, grep-matcher, thiserror)
$ VR_SEARCH_BENCH_ROOT=/usr/share (81 403 files) ... --release -- --ignored --nocapture project_search_first_batch_latency
  term "license":   first batch 60.0 ms, 515 batches, 12430 files, 100000 matches (limit Error), done 2.14 s   (cold cache, first run)
  term "Copyright": first batch 3.6 ms, 526 batches, 33790 files, 100000 matches (limit Error), done 3.37 s
  term "zqxjvunlikely": 0 batches, 41045 text files searched, done 1.13 s (full walk)
  ~/.cargo/registry/src (10 458 files) term "fn": first batch 1.3 ms, done 20 ms (limit hit)
```
Guard (BASE-25):
```
$ cargo fmt --all --check            -> rc=1, diffs only in crates/vr-core/src/document.rs, crates/vr-core/src/watch.rs (T4 in progress) — not mine
$ cargo clippy --workspace --all-targets -- -D warnings
error: using `chunks_exact` with a constant chunk size --> crates/vr-core/src/document.rs:372:23 (T4 in progress) — not mine
  → per brief: `cargo clippy -p vr-search --all-targets --no-deps -- -D warnings` clean (--no-deps because -p alone also lints the path dependency vr-core and hits the same T4 error)
```

## Deviations from the plan (with reason)
- Plan says "WalkParallel + grep-searcher". WalkParallel: yes. grep-searcher is used as the candidate-line finder only; every candidate line is re-matched by the exact per-line matcher, and files with a lone `\r` (or patterns with `\A`/`\z`, or patterns grep-regex rejects) use our own D31 line split. Reason: grep-searcher counts lines by `\n` only, which disagrees with D31 on lone-CR files.
- Contract additions (BASE-2 additive): `FilePreview` has private `original`/`replaced` bytes and a `replacements()` accessor; `Hunk { line, before, after }` defined here (the plan named `FilePreview` without fields); `SearchMsg::Error` is non-terminal, `Done` always last; `search_project(root: &Path, q: &Query, …)` and `plan_replace(root: &Path, q: &Query, replacement: &str, files: &[PathBuf])` take references.
- Batching: ≤200 matches or 50 ms after the first match of a batch (plan: "≤ 200 matches or every 50 ms").
- vr-fs walk policy copied (vr-fs is not a dependency of vr-search and its builder is private): hidden(false) parents(false) ignore(false) require_git(false) follow_links(false), `.git` filtered — project.rs `walk_builder`.
- BASE-3: tests for find whole-word/captures and the first project and replace tests were red before code; the remaining project/replace tests were written after their implementation and passed at first run — red shown by mutation instead (lines above).

## Out-of-scope issues noticed
- crates/vr-core/src/document.rs (T4, uncommitted): clippy `chunks_exact_to_as_chunks` error at :372 and rustfmt diffs in document.rs/watch.rs break the workspace guard.
- ropey still has `unicode_lines` on (D31 pending in T4); vr-search does not depend on rope line APIs, so it is unaffected.
- apply_replace keeps mode but not owner/group (would need chown/rustix, not in vr-search's manifest).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| ignore-aware walk (D15) | capabilities.md, crates/vr-fs/src/file_index.rs:158 | vr-fs builder is private and vr-fs is not a vr-search dependency → same config copied in project.rs `walk_builder` |
| Transaction / Change / one-undo | crates/vr-core/src/{transaction,text,lib}.rs | reused `vr_core::{Change, Transaction}`, tests use `vr_core::Buffer::apply/undo` and `transaction::apply_changes` |
| atomic write | capabilities.md (vr-core document save is T4, uncommitted), vr-fs ops (no write helper) | new `write_atomic` in replace.rs (private) |
| line split by D31 | vr-core text.rs (`is_line_break`, rope-based) | new `line_spans(&str)` — works on &str/bytes for files, not on a rope |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | `git status --short crates/` → only crates/vr-search/src/{lib,find,project,replace}.rs are mine; no Cargo.toml/Cargo.lock/vr-core/vr-fs edits |
| BASE-2 | pass | lib.rs re-exports `find_all`, `replace_all_tx`, `ProjectMatch`, `SearchMsg`, `search_project`, `FilePreview`, `plan_replace`, `apply_replace` with plan field names/types; additive items listed under Deviations |
| BASE-3 | pass | red lines per test in Progress (todo!/E0599/assertion), mutation-red for tests written after code |
| BASE-4 | pass | trimmed outputs above |
| BASE-5 | pass | release probe build via `$TEAM_BIN/run-heavy.sh cargo test -p vr-search --release`; nothing left running |
| BASE-6 | pass | comments explain why (e.g. project.rs `prefilter` doc, replace.rs `apply_one` race comment); no bug history |
| BASE-10 | pass | only `cargo test -p vr-search` (+ filtered runs) |
| BASE-16 | pass | `## Reuse` table; capabilities.md vr-search section added |
| BASE-19 | pass | this file, one progress line per step |
| BASE-25 | pass | workspace guard fails only in vr-core T4 files (not mine); `cargo clippy -p vr-search --all-targets --no-deps -- -D warnings` clean, `cargo fmt -p vr-search --check` clean |
| BASE-32 | pass | D15 → `project_search_respects_gitignore` (no repo, hidden included, .git skipped); D31 → `project_search_line_numbers_follow_d31`, `find_regex_multiline_off`; D24/D25 spirit → `apply_replace_skips_file_changed_since_preview` |
| BASE-38 | pass | `rustfmt --edition 2024 crates/vr-search/src/{lib,find,project,replace}.rs` only |
| COST-8 | pass | files written whole, edits by perl -pi / Edit; no python heredocs |
| COST-10 | pass | related set = vr-search tests only |
| AC-9 | pass | `project_search_respects_gitignore` (.gitignore at root and subdir, *.log, target/ excluded) |
| AC-10 | pass | `replace_all_regex_captures`: `fn (\w+)` → `fn new_$1`, 3 changes in one Transaction, one `undo()` restores, second `undo()` false |
| AC-11 | pass | `project_search_streams` (≥10 batches ≤200, Batch before Done, line 2 col 1), `forward_batches_flushes_on_timer_while_walk_runs`; probe: first batch 1.3–60 ms on 10k–81k-file trees; Enter target = `path`+`line`+`col` (1-based) |
| AC-12 | pass | `plan_replace_previews_hunks` (per-file hunks, nothing written), `apply_replace_only_confirmed_files`, `apply_replace_keeps_crlf` (CRLF, BOM, lone CR, no final newline), `plan_replace_refuses_unsafe_files` (non-UTF-8 kept), `apply_replace_skips_file_changed_since_preview`; open-buffer routing is vr-ide's (T10) via `replace_all_tx` |
| RF-4 | pass | streaming + cancel: `project_search_streams`, `project_search_cancel_stops`; long lines windowed `project_search_long_line_is_windowed`; 100k match cap |
| RF-5 | pass | `replace_all_zero_length_no_loop` (`^`, `$`, `x*`, empty regex on Vietnamese), one undo asserted |

n-a: BASE-8 (no dependency added; manifest untouched) · BASE-11, BASE-24 (no git write commands run) · BASE-14 (every command finished in the foreground limit) · BASE-15, COST-9 (no background task started; nothing left running) · BASE-17 (no installs) · BASE-18 (no sub-agents) · COST-13 (no throwaway scripts beyond scratchpad snippet files; no tool search) · COST-15 (no goldens/snapshots)

## Fix round 1
Input: 05_review_T9.md (3 major, 5 minor). T4 (vr-core document save) is still uncommitted → write path fixed locally (BASE-16 follow-up below).
- read review, decisions D33–D45, vr-core/document.rs temp naming (`.vr-ide-save-*.tmp`, read-only refusal D41), vr-fs rustix renameat2 usage.
- M1 red `commit_keeps_agent_write_made_before_the_swap`: error[E0425] cannot find function `write_temp` / `commit_replace` → green after write_temp (fsynced temp) + commit_replace (precheck, renameat2 EXCHANGE, compare swapped-out bytes, swap back on mismatch; fallback re-check right before rename). rustix added to crates/vr-search/Cargo.toml (D45). Branch proof by mutation: swapped-out compare forced true → FAILED replace.rs:554; fallback re-check removed → FAILED replace.rs:551.
- reds (tests written first; compile stubs `Limits`/`search_project_with`/`plan_replace_with`/`TEMP_SEQ` added so they run):
  - M2 `project_search_skips_oversized_files`: E0422 `Limits` / E0425 `search_project_with`, then left ["big.sql","small.txt"] right ["small.txt"]
  - M2 `plan_replace_refuses_oversized_files`: E0425 `plan_replace_with`, then panicked replace.rs:579 (no error on big file)
  - M3 `project_search_prefilter_never_drops_matches`: left 0 right 1 (`foo(?-m)$`)
  - minor `replace_digit_group_before_word_char`: left "fn " right "fn alpha_old"
  - minor `whole_word_agrees_with_word_boundaries`: left [(0,3),(6,9)] right [(6,9)] (NFD mark)
  - minor `apply_replace_refuses_read_only_and_hard_linked_files`: panicked replace.rs:597 (0444 file written)
  - minor `apply_replace_rechecks_root_and_dedupes_links`: panicked replace.rs:626 (link to a.txt applied twice / no "same file as")
  - minor `project_search_lists_a_linked_file_once`: left ["a.txt","ext.txt","link.txt"] right ["a.txt","ext.txt"]
  - minor `write_temp_never_touches_existing_names_and_fits_long_names`: E0425 `TEMP_SEQ` only — the short `.vr-ide-save-PID-sN.tmp` name and the AlreadyExists retry were already written in the M1 step; mutation proof below.
- green: replace.rs rewritten head (fingerprint + Spec instead of file bytes, `plan_replace_with` cap, `apply_one` root re-check / same-file dedupe / read-only / hard-link refusal, recompute from re-read bytes); project.rs `Limits`, `read_capped` (never allocates more than cap+1), `size_text`, `WalkCtx` + `visit` (oversized counted → one `Error` before `Done`; link to an in-project file skipped), `prefilter_safe`; find.rs whole word via `\b{start-half}(?:pat)\b{end-half}` (raw pattern compiled first), `numbered_groups_braced` for `$1_old`.
- mutation reds (copy, mutate, run, restore by cp):
  - `find_regex_multiline_off`: line break only `\n` → FAILED left [(9,12)] right []
  - `replace_all_zero_length_no_loop`: drop last_end guard → FAILED left "-a--b-" right "-a-b-"
  - `replace_all_unicode_offsets`: byte offsets instead of byte_to_char → FAILED left [(27,31),(37,41)] right [(16,20),(24,28)]
  - `write_temp_never_touches_existing_names_and_fits_long_names`: delete the existing name on AlreadyExists → FAILED replace.rs:766
  - `project_search_lists_a_linked_file_once`: drop in-project-link skip → FAILED left [..,"link.txt"]
- clippy: `cloned_ref_to_slice_refs` in a test → `std::slice::from_ref`.

### Per finding
- M1 replace race — fixed. write_temp (perm before content, fsync) → commit_replace: re-check → `renameat2(RENAME_EXCHANGE)` → compare swapped-out bytes → swap back + "changed on disk" on mismatch; fallback (no exchange: non-Linux, EINVAL fs) re-checks right before rename. Dir fsync after success. Comment in replace.rs `commit_replace` states the window. rustix added to crates/vr-search/Cargo.toml (D45); Cargo.lock gains only `"rustix"` in vr-search's dependency list.
- M2 unbounded reads — fixed. 32 MB per-file cap (`MAX_FILE_BYTES`) for search and replace via `read_capped`; search counts skipped files and sends `Error("N files larger than 32 MB not searched")` before `Done`; previews keep a (len, SipHash) fingerprint instead of two copies of the file, apply re-reads (capped at the previewed length) and recomputes. Memory bound: walker threads (ignore default ≤ 12) × 32 MB. Cancel: checked per file and per line; a capped read is ≤ 32 MB.
- M3 prefilter misses `(?-m)` — fixed. `prefilter_safe`: any `(?` other than `(?:`, `(?<`, `(?P<` (and `\A`/`\z`) → exact per-line path. Test covers `foo(?-m)$`, `(?-m)^foo`, `(?m)foo$`, `(?s)foo.`, `(?i)FOO` against `find_all`.
- minor `$1_old` — fixed (my call): `$<digits>` always means the numbered group (`$1_old` → group 1 + `_old`, VS Code behaviour); `$$` escape, `${..}` and `$name` unchanged.
- minor whole word vs `\b` — fixed with regex half boundaries (Unicode `\w`, marks included; alternation resolved inside the engine); raw pattern validated first so wrapping cannot make `a)(b` valid.
- minor write_atomic — fixed: short temp name `.vr-ide-save-PID-sN.tmp` (T4's prefix, so `is_save_temp` will match once T4 lands); AlreadyExists → next name, never removes a file it did not create; dir fsync; hard-linked files refused; read-only (no write bits) refused with PermissionDenied (D41); canonical root re-checked at apply.
- minor link listed twice — fixed in search (a symlink whose target is inside the root is skipped; links pointing outside are kept) and in apply (same canonical file → "same file as <first path>; replaced once").
- minor BASE-3 — mutation reds recorded above.

### For decisions.md (leader numbers)
- D? (T9) Replace race window — `renameat2(RENAME_EXCHANGE)` swap, compare swapped-out bytes, swap back on mismatch; remaining window: a writer still holding the old file open and writing between that comparison and the unlink of the old file. Without exchange (non-Linux, filesystems answering EINVAL) the re-read happens right before rename(2); a write between re-read and rename is lost. Like D32.
- D? (T9) File size cap — files > 32 MB are not searched (counted, one Error before Done) and not replaced (error item); previews keep a (len, SipHash) fingerprint, not file bytes — memory bounded by threads × 32 MB.
- D? (T9, amends D36) `$<digits>` in a regex replacement always names the numbered group (`$1_old` = group 1 + `_old`), unlike the regex crate's longest-name rule.
- D? (T9) Whole word = no Unicode `\w` char immediately before or after the match (regex `\b{start-half}`/`\b{end-half}`).
- D? (T9) Project search lists a symlinked file once: a link whose target is inside the project is skipped (known gap: if that target is gitignored, the link's matches are not shown); links pointing outside are searched. Replace refuses read-only and hard-linked files and re-checks the root at apply.

### BASE-16 follow-up
T4's atomic save (vr-core document.rs) is still uncommitted, so replace keeps its own `write_temp`/`commit_replace`. Once T4 lands: share one helper (or move the exchange-verify logic into vr-core) so save and replace have one temp scheme and one race policy.

### Fix round 1 evidence
```
$ cargo test -p vr-search
test replace::tests::commit_keeps_agent_write_made_before_the_swap ... ok
test project::tests::project_search_skips_oversized_files ... ok
test replace::tests::plan_replace_refuses_oversized_files ... ok
test project::tests::project_search_prefilter_never_drops_matches ... ok
test find::tests::replace_digit_group_before_word_char ... ok
test find::tests::whole_word_agrees_with_word_boundaries ... ok
test replace::tests::apply_replace_refuses_read_only_and_hard_linked_files ... ok
test replace::tests::apply_replace_rechecks_root_and_dedupes_links ... ok
test replace::tests::write_temp_never_touches_existing_names_and_fits_long_names ... ok
test project::tests::project_search_lists_a_linked_file_once ... ok
test result: ok. 31 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.11s
$ cargo clippy -p vr-search --all-targets --no-deps -- -D warnings
    Checking vr-search v0.1.0 — no vr-search diagnostics (only vr-core dead-code warnings from T4's uncommitted document.rs)
$ cargo fmt -p vr-search --check  -> clean
$ git diff Cargo.lock  -> +"rustix" in vr-search's dependency list only
```

### Rule compliance (round 1 changes)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | changed: crates/vr-search/src/{find,project,replace}.rs, crates/vr-search/Cargo.toml (+rustix), Cargo.lock (+1 line) |
| BASE-3 | pass | red lines per new test above; mutation reds for the three older tests and write_temp |
| BASE-8 | pass | rustix approved in D45 (workspace dep already present) |
| BASE-16 | pass | T4 save uncommitted → local fix + follow-up noted |
| BASE-32 | pass | D25: `commit_keeps_agent_write_made_before_the_swap` (exchange + fallback); D41: read-only refusal test |
| RF-4 | pass | `project_search_skips_oversized_files`, `plan_replace_refuses_oversized_files`, `read_capped` |
| AC-11 / AC-12 | pass | prefilter parity test; apply refusals tests |
Other rows unchanged from the first table.

## Fix round 2
Input: 05_review_T9.md `## Round 1 re-check` (1 major: swapped-out file deleted when swap-back or post-swap verify fails; minors: `(?x)` comment + whole word, hard-link message per D48, decision lines).
- tests first: `commit_keeps_other_version_when_swap_back_fails` (seam: 2nd swap fails), `commit_keeps_other_version_when_verify_fails` (seam: swapped-out file made unreadable, stands in for EMFILE/EIO), `whole_word_with_extended_mode_comment`, hard-link test asserts "open the file and save it"; `commit_replace` takes a `swap` seam instead of `exchange: bool` (production passes `exchange_files`).
- reds: first E0308 (seam signature) ×4; after the signature-only change:
  - swap_back_fails: panicked replace.rs:804 — error "…the other version is at /tmp/.tmpBxGvbE/.vr-ide-save-658626-s1.tmp" (no "kept at", file already deleted)
  - verify_fails: panicked replace.rs:804 — "Permission denied (os error 13)" (plain error, swapped-out file deleted)
  - whole_word_with_extended_mode_comment: panicked find.rs:469 (InvalidRegex unclosed group)
  - refuses_read_only_and_hard_linked_files: panicked replace.rs:732 (message lacks "open the file and save it")
- green: `commit_inner` returns `(result, TmpHolds::{Ours, Unknown(what)})`; `commit_replace` deletes `tmp` only for `Ours`; for `Unknown` `keep_conflict` hard-links it to `<name ≤200 bytes>.vr-ide-conflict-PID-N` (never replaces an existing name; falls back to leaving the temp path) and the error ends with "kept at <path>". Whole word: if the wrapped pattern fails while the raw one compiled, retry with a newline before the wrapper's `)` (ends an `x`-mode comment). Hard-link message per D48.
- mutation: delete instead of keep → both commit_keeps_other tests FAILED (replace.rs:905, :937); restored.
- evidence:
```
$ cargo test -p vr-search
test replace::tests::commit_keeps_other_version_when_swap_back_fails ... ok
test replace::tests::commit_keeps_other_version_when_verify_fails ... ok
test find::tests::whole_word_with_extended_mode_comment ... ok
test replace::tests::apply_replace_refuses_read_only_and_hard_linked_files ... ok
test result: ok. 34 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.07s
$ cargo clippy -p vr-search --all-targets --no-deps -- -D warnings
    Checking vr-search v0.1.0 … Finished (no diagnostics)
$ cargo fmt -p vr-search --check -> clean
```
- Rule rows touched: BASE-3 pass (reds above), BASE-32 pass (D25: other writer's version never deleted — two tests; D48 message), BASE-6 pass (commit_replace comment states the full remaining window incl. a second write between swap and swap-back). Others unchanged.

## Decisions to log
- D? (T9) Replace race window: renameat2(RENAME_EXCHANGE), compare the swapped-out file, swap back on mismatch. Still unprotected: a writer that holds the old file open and writes between our compare and the unlink, or a second write (e.g. a rename over the target) between our swap and the swap-back. Without EXCHANGE (non-Linux, EINVAL filesystems), a write between the re-read and rename(2) is lost (like D32). If the swap-back or the post-swap read fails, the swapped-out file is kept as `<name>.vr-ide-conflict-PID-N` and the error names it; never deleted.
- D? (T9) Files over 32 MB are not searched (counted, one Error before Done) and not replaced (error item). Previews keep only (len, SipHash), not file bytes. Apply re-reads and compares full bytes at commit. Memory is bounded by walker threads × 32 MB.
- D? (T9, amends D36) In a regex replacement, `$` + digits always means the numbered group (`$1_old` = group 1 + `_old`, VS Code behaviour). `$$`, `${..}` and `$name` keep regex-crate meaning.
- D? (T9) Whole word means no Unicode `\w` char (marks included) right before or after the match. It is built with `\b{start-half}`/`\b{end-half}` inside the regex, so `ab|abc` finds `abc`. An `x`-mode trailing comment is closed with a newline.
- D? (T9) Project search lists a symlinked file once: a link whose target is inside the project is skipped, and links pointing outside are searched. Known gap: a gitignored in-project target reached only through a link is not shown. Apply dedupes confirmed paths by canonical file ("same file as …").
