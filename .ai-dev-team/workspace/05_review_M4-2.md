# Review M4-2 — highlighter engine (round 0)

Verdict: **CHANGES_REQUESTED** (2 major, 4 minor)

Proof: probe crate `qa-artifacts/M4-2/review-probe/` (path dependency on the worktree, own target dir, release), output in `qa-artifacts/M4-2/review-probe/probe.log`.

## Findings

[major] crates/vr-syntax/src/highlighter.rs:539, :689, :805 — when an injection's language changes but its content does not, the old embedded layer stays (AC-13a/b)
Scenario: Markdown "```rust\nfn main() { … } // c\n```", the user changes `rust` to `python`. `Record::touches` checks only the content ranges. The info string ends one byte before `code_fence_content`, so the old `rust` record is kept in `absorb` and again in `root_injections`. The region query then adds a `python` record for the same range. Both layers get parsed and painted, so Rust colours (`()` Punctuation 34..36, `// c` Comment 56..57, …) mix with the Python ones. The result does not match a full parse (probe: `PROBE1 equal=false`) and stays wrong until a `reset`. The same path applies to Vue `<script lang="ts">` → `lang="js"` and to any HTML/Vue injection chosen by an attribute captured outside the content. The random-edit test does not catch this because its edits seldom fall on an info string or an attribute value.
Fix: store each record's match extent (the union of the ranges of every captured node in the match, including `@injection.language` and `@_lang`/`@_type` captures) and use it in `touches` in both places. Add a test that edits a fence info string and a Vue `lang` value, then compares the result with a full parse.

[major] crates/vr-syntax/src/highlighter.rs:526-551 — the cost of a many-change transaction grows with changes × embedded layers, so colours stay frozen for seconds
Scenario: a 175 KB Markdown file with 4000 paragraphs (4000 inline layers) and a replace-all of 4000 words, sent through `Edit::from_changes` → `edit`. The reparse lands after **7.04 s**, against 0.16 s for a fresh full parse (probe `PROBE2`). For each edit, `absorb` calls `tree.edit` on every old layer and shifts every entry of `edited` (O(n²)). It also retains and shifts every root record. None of this is bounded by the parse timeout or cancellable. During that time, every replaced word and everything that changed meaning stays plain or stale. The cost grows quadratically with larger files or transactions. A small `spans()` window stayed cheap (6.3 ms).
Fix: in `absorb`, when a message has more than N edits (for example 32), collapse them into one covering edit (first start .. last old end → new end, with points from the ropes). The trees, layers, records and `edited` then take one edit. Alternatively, apply each edit only to the layers and records at or after it, and let `Msg::Reset` handle very large lists. Add a test that times a 1000-change transaction against a full parse.

[minor] crates/vr-syntax/src/lib.rs:121-123 — the exception list for first-match-wins precedence is a hard-coded `matches!(self.id, "go" | "json")`
Scenario: a grammar added later whose crate query is written first-match-wins (as Go's is) gets the wrong colours, and nothing in the grammar table or the tests forces anyone to decide its precedence.
Fix: add a required `precedence: First | Last` field to the `grammar!` macro next to the query sources, plus one inventory test per grammar that asserts a known specific-over-generic capture.

[minor] crates/vr-syntax/src/registry.rs:132-135, :153-155 — a language the user adds cannot claim an extension or alias that a built-in language already owns, and no warning is given
Scenario: the user adds `[jsonc] grammar = "json"` with `extensions = ["json"]`. The built-in `json` entry comes first and keeps `.json`, and no warning says so. (Editing `[json]` itself works.)
Fix: either let entries from the user file win ties (search them in reverse order), or report a duplicate claim as `InvalidValue`.

[minor] crates/vr-syntax/src/edit.rs:108-115 — `point_at` uses ropey's `cr_lines` line index (a lone CR breaks the line), while `advance` and tree-sitter count only LF
Scenario: in a file with bare `\r` line endings, edit points and the root layer's `end_point` disagree with tree-sitter's own points. Byte offsets stay right, so colours are unaffected, but `Range` equality in `layers()` (line 643) fails, and the layer is reparsed instead of reused. Only performance suffers.
Fix: compute points by counting `\n` in the rope chunks, or accept this and note it in D161.

[minor] crates/vr-syntax/src/highlighter.rs:813-831 — injection queries run without a time limit and cannot be cancelled
Scenario: the first full root injection query on a 10 MB file, and the per-cycle whole-layer queries of nested layers, ignore `cancel`. After a drop or `reset`, the old thread keeps working for up to about 0.8 s (the notes' numbers). Nothing blocks the UI.
Fix: give the `QueryCursor` a progress callback that checks `cancel`.

Checked and sound:
- Lock use. The mutex is held only to swap or clone an `Arc`, and `Notify` is called outside it, so a notifier that reads `status()`/`spans()` cannot deadlock.
- Drop. `drop` sets `cancel` and closes the channel, and never joins.
- Reset. A new `Shared` is created, so the old worker cannot publish into it.
- The channel is unbounded but drained in full before every cycle.
- Versions. The `pending` filter, `map_back` and `map_forward` hold for edits inside, across and at the edges of spans. Pure insertions at a span's start or end leave the inserted bytes plain.
- `from_changes` puts the last change first. `diff` keeps both ends on char boundaries, and the prefix and suffix cannot overlap.
- A timed-out `spans()` returns a partial but coherent result from an immutable snapshot.
- `spans()` never parses.
- Size limit. `max_bytes` is checked on new, edit and reset.
- The deviation from tree-sitter-highlight (D160) is justified.

## Files read
- crates/vr-syntax/src/highlighter.rs — read
- crates/vr-syntax/src/edit.rs — read
- crates/vr-syntax/src/query.rs — read
- crates/vr-syntax/src/capture.rs — read
- crates/vr-syntax/src/registry.rs — read
- crates/vr-syntax/src/lib.rs (diff) — read
- crates/vr-syntax/Cargo.toml (diff) — read
- crates/vr-config/src/languages.rs — read
- crates/vr-config/src/lib.rs (diff) — read
- crates/vr-config/defaults/languages.toml — read (header + first tables)
- crates/vr-syntax/tests/highlight.rs — read (test list, random-edit test 150-265)
- crates/vr-config/tests/languages.rs, crates/vr-syntax/tests/registry.rs — test names only
- Cargo.lock — skipped (lockfile; 4 dependency edges, no new packages per notes)

## Rule check M4-2
| Rule | Result | Note |
|---|---|---|
| BASE-1 | pass | `git status`: only Cargo.lock, crates/vr-syntax/**, vr-config lib.rs + languages files; keymap.toml and tests/config.rs untouched |
| BASE-2 | pass | `Capture` (15 classes) as in the plan; additions listed under the notes' "## API" section |
| BASE-3 | pass | Tests written after the code are listed. Those that matter: query.rs `lua_patterns_become_regexes` (predicate correctness for nvim queries), `spans_call_is_bounded_by_the_query_timeout` (the 50 ms contract), `edits_after_a_timeout_stay_plain_and_reset_tries_again`. Glob and rope-chunk tests are low risk |
| BASE-8 | pass | only workspace dependencies added (ropey, regex, vr-config, dev tempfile); root Cargo.toml unchanged |
| BASE-25 | pass | fmt + clippy (default and no-default-features) output in the notes |
| New-states rule | pass (gap) | Table present; the language-change and large-transaction cases are missing (findings 1 and 2) |

## Round 1 re-check

Verdict: **APPROVED**. Both majors and all four minors are closed. One new minor is not blocking.

Probe: same crate, rebuilt against the round-1 worktree (release); output in `qa-artifacts/M4-2/review-probe/probe-r1.log`.
- Major 1 (injection language changed): closed. `PROBE1 equal=true`: after the `rust` → `python` fence edit, the spans equal a fresh parse. `Record.extent` covers every captured node, and both drop points use it (`apply_queued` :580, `root_injections` :756).
- Major 2 (many-change transaction): closed. 4000 changes over 4000 inline layers: reparse 238 ms, against 151 ms for a fresh parse (was 7.04 s). `spans()` while pending: 5.3 ms.
- Precedence: closed. `Precedence` is a required `grammar!` argument (lib.rs:336), with one test case per grammar.
- Claims: closed. `take_claims` (languages.rs:149) lets the table merged last win. No two built-in languages share a name in the defaults (checked), so merging in alphabetical key order changes nothing.
- Lone CR: closed. `points_at` counts LF only.
- Injection queries: closed. `matches_with_options` with cancel and deadline; a root timeout ends as `Disabled::Timeout`, a nested one keeps that layer's colours.

New logic checked:
- `covering_edit` (highlighter.rs:825): traced edits that lie before, inside, across and after the covered range, including gaps. Start, old end and new end stay correct in both coordinate systems, and the points come from `applied` and `text`. The caller's `pending` list keeps the original edits, so `spans()` mapping is unaffected. `Reset` clears `queued` and sets `applied`, so no edit from the old text leaks into the new one.
- `OldLayer.dirty` is tested (inclusive) before `edit_range`. A clean layer is reused only when its range bytes are equal. Combined layers whose gaps were edited reuse correctly, because their ranges shift together.
- One-byte widening: retaining a kept record against a widened half-open region equals an inclusive touch on the original region. A found record replaces kept ones whose content overlaps it. In the shipped injection queries, root-level contents do not nest (Vue interpolation, directive attribute, script/style, Markdown fence/html_block), so no valid kept record is lost. The oracle (20 samples × 200 edits) agrees.
- `Reparse::Full` (markdown_inline): a dirty layer is parsed without a base tree. A clean layer keeps its old tree, which is only shifted by `tree.edit` and never reparsed incrementally, so the grammar bug in incremental reparse cannot reach it. Sound. The cost is per paragraph.
- No new lock or channel. Folding happens on the worker only. Drop and reset are unchanged.

[minor] crates/vr-syntax/src/edit.rs:134-160, :101-106 — `point_at` is now a linear pass from byte 0, run on the caller's thread
Scenario: `Edit::from_chars` costs 3–6 ms per keystroke on a 5–10 MB file (the notes' numbers). `Edit::diff` (undo/redo) calls `point_at` three times, each pass starting again from 0: about 3 × 6 ms ≈ 18 ms on a 10 MB file, which can skip a frame on undo. Smaller files are unaffected.
Fix: in `diff`, call `points_at(old, &[prefix, old_end])` once. Optionally use ropey's line index and fall back to counting only when the rope contains a lone CR.

## Rule check M4-2 (round 1)
| Rule | Result | Note |
|---|---|---|
| BASE-1 | pass | Only crates/vr-syntax/** and vr-config languages files changed |
| BASE-2 | pass | `Precedence` and `Grammar::precedence()` additions are listed; no contract changed |
| BASE-3 | pass | A red line per new test; `many_edits_fold_into_one_covering_edit` written with its function, its behaviour's red is the 4000-change test |
| BASE-8 | pass | No dependency added |
| BASE-25 | pass | fmt + clippy (default, no-default-features, rust-only) in the notes |
| New-states rule | pass | Round 1 table covers queued/folded edits, reset during queued edits, and an edited injection language |

## Round 2 re-check

Verdict: **APPROVED**. No new staleness or race. The r1 minor is closed. Two new minors follow, neither blocking.

Checked (highlighter.rs:139-177, :520-602, :675-695; edit.rs `diff`):
- **Edits during a verify parse.**
  - `edit` stores `Shared.latest` before it sends the message, so the worker always gets a message for a version that stopped its check.
  - `Stop::Superseded` restores the saved tree, old layers and records. `queued` and `edited` are empty during `verify`, because `cycle` already consumed them and nothing is absorbed in between.
  - The loop then `continue`s to `recv`, which already holds the new edit.
  - If the fresh parse wins the race against a new edit, it publishes version V for V's text, and the caller's `pending` entries above V still map it forward. No edit is lost.
  - `edits_during_the_fresh_parse_check_are_not_lost` exercises this path.
- **Parsing → Ready.** Both publishes carry the same version and text. Only `exact` differs, and it gates `status`/`wait` but not `spans()`. So the provisional spans describe exactly the text they were computed for, plus the usual shifting through pending edits.
  - `mark_exact` stores the flag under the lock, so `wait` cannot miss the notify.
  - A `mark_exact` on an `exact` Arc that has since been replaced is harmless.
  - A superseded check leaves the older snapshot inexact, but `ready_at` ignores older versions anyway.
  - Disabled, cancel and reset behave as before: a new `Shared` per reset, and `publish` is a no-op once disabled.
- **`Edit::diff`.** One `points_at` pass over `old`. The new end point is `advance`d over the inserted slice only, which is correct for LF, CRLF and lone CR. This closes the r1 minor.

[minor] crates/vr-syntax/src/highlighter.rs:541-563 — CPU cost of the error check
Scenario: a file of at most 1 MiB that keeps a syntax error gets a full parse of every layer after every cycle. This happens in normal typing, and permanently in templates such as Blade/Twig/Vue or code the grammar does not cover.
- While typing, each check is stopped at the next keystroke, so roughly one core stays busy for the active document.
- At each pause, a full parse runs: about 90–150 ms at 200 KB and about 0.5–1 s near 1 MiB, on the worker.
- There is no impact on the UI or on time to colour, because the incremental spans are published first.
- Verdict: acceptable for one active document; this is a battery/CPU cost only.
- Cheaper options:
  - (a) run the check only when a layer whose grammar is flagged unstable has an error (php_only, twig, markdown_inline, as a `Grammar` field next to `Reparse`);
  - (b) lower `VERIFY_MAX_BYTES` to about 256 KiB;
  - (c) start the check only after a short idle delay (for example 150 ms with no new message, via `recv_timeout`), so typing bursts never start one.
- (c) alone removes most of the waste. Leader's call.

[minor] crates/vr-syntax/tests/highlight.rs:587-598 — test cost
Scenario: the default `cargo test -p vr-syntax` runs 7 fuzz passes, which take 133 s in debug. That is most of the crate's test time, on every developer and QC run.
Fix: by default run 2 seeds that found real bugs (987654321 and 42) plus 1 transaction seed. Move the full set behind `VR_SYNTAX_FUZZ_SEEDS=all`, or into an `#[ignore]`d test that QC runs in release (40 s). The regressions stay covered, because both failing seeds stay in the default run.

## Rule check M4-2 (round 2)
| Rule | Result | Note |
|---|---|---|
| BASE-1 | pass | Only crates/vr-syntax/** changed |
| BASE-2 | pass | No public API change |
| BASE-3 | pass | Oracle red first (r2-red-oracle.log); 2 guard tests written after the fix are listed |
| BASE-8 | pass | No dependency added |
| BASE-25 | pass | fmt + clippy in the notes |
| New-states rule | pass | The round 2 table covers drop, failure, edits during the check, and reset during the check |

## Quick pass 1 re-check

Verdict: **APPROVED**. Both round-2 minors are closed, and the new wait state has no race and loses no edit.

Diff `git diff 84c407d` (highlighter.rs serve loop + `verify_idle`; tests/highlight.rs) read in full.

- **Idle wait** (`serve`, `recv_timeout(verify_idle)`):
  - A message that arrives during the pause is carried in `next` and absorbed first, then anything else queued is drained. No edit is lost, and no check runs for the replaced snapshot.
  - `Disconnected` (drop, or reset replacing the link) returns at once.
  - `cancel` is checked again before `verify`.
  - The provisional snapshot stays published with `exact = false`, so `status`/`wait` stay Parsing until the check or a newer cycle lands; nothing gets stuck.
  - Edits during the check itself still take the r2 Superseded path, and `edits_during_the_fresh_parse_check_are_not_lost` now uses `verify_idle: 0` so that path stays exercised.
- **`from_scratch` marked exact:** sound.
  - The condition is: no tree, no old layers and `root_records` None. That holds only before the first cycle or after a `Reset`.
  - After any cycle, `root_injections` sets `root_records = Some(..)` even when the language has no injections, so a later cycle is never mistaken for a fresh one.
  - Such a cycle parses the root without an old tree, parses every layer without a base, and runs the full injection query. That is exactly what a new `Highlighter` does, so a check would only repeat it.
- **Test cost:**
  - The default oracle keeps the two seeds that found bugs plus one transaction seed: 50.7 s, down from 132.9 s.
  - The full set is `#[ignore]`d with a release command that QC can run.
  - `the_fresh_parse_check_waits_until_typing_pauses` tolerates sleeps that overshoot under load.

Latency (release, `perf_edit_to_spans*`, `--test-threads=1`):
- **Conditions:** at 01:22. `run-heavy --status` showed slot 1 busy with another project's e2e run (/home/rainy/DEV/vr-talex: Chrome plus gnome-shell headless, about 1 core of 8) and slot 2 free. The M4-3 debug build had finished before the run. Load average went from 7.1 to 4.4 during the run.
- **Log:** qa-artifacts/M4-2/qp1-review-perf.log.

| Case | Initial parse | Edit → spans avg / worst | `spans()` worst | `Edit::from_chars` worst |
|---|---|---|---|---|
| 200 KB Rust | 63 ms | 9.8 / 14.6 ms | 1.7 ms | 0.10 ms |
| 200 KB HTML + script/style | 167 ms | 14.5 / 17.8 ms | 1.9 ms | 0.11 ms |
| 5 MB Rust | 1.44 s | 317 / 369 ms | 2.3 ms | 3.0 ms |
| 10 MB single-line JS | 3.69 s | 1.20 / 1.49 s | 0.46 ms | 6.1 ms |

These match r2 (10.8/16.3, 19.3/29.5, 318/425 ms, 1.23/1.51 s) or are slightly better, so the quick pass caused no regression. The developer's higher "after" column came from running beside the full fuzz test.

No findings.
