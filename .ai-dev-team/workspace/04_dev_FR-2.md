# FR-2 notes

Worktree: .worktrees/FR2 (run4-FR2 from ca60805)

## Progress
- started; reading changes.rs
- red tests written (d27, sec16 clean_dirs, sweep race, row counts); scaffolds no-op
- fixes applied for a,b,c,d; FR-2 tests green (7/7)
- 2 goldens updated (row counts +9 -> +10, the item-d fix); running crate + clippy
- crate run, fmt and clippy green

## Files changed
- crates/vr-ide/src/ai/changes.rs
- crates/vr-ide/src/ai/changes_tests.rs (4 new tests; 2 existing tests use `app.toml` / `notes.txt` instead of `.env` as their revertable sample, since `.env` is now never copied)
- crates/vr-ide/src/ai_changes_tests.rs (row-count assertion in both refused tests)
- crates/vr-ide/src/ui/tests/snapshots/…ai_revert_refused_80x24.snap, …ai_revert_created_80x24.snap (golden only: row `+9 -3` -> `+10 -3`, the stale count item d fixes; no ui/*.rs touched)

## Items (red -> change -> green)
### a. D27 baseline skips secrets and other mounts
- Red: `d27_secrets_and_other_mounts_are_listed_but_never_copied` -> `left: 11, right: 1` (11 blobs, incl. .ssh/id_ed25519, .env, mnt/plain.txt).
- Change: `State::no_copy(rel, meta)`: other st_dev than the root, any path component matching `id_*`, `*.pem`, `*.key`, `.env*`, `*credentials*` (case-insensitive), or below a secret folder (`~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.netrc`, `~/.config/gh`, `~/.claude`, `~/.claude.json`, `~/.codex`, `$CLAUDE_CONFIG_DIR`, `$CODEX_HOME`, canonical). Used by `baseline_file` (walk) and `rebase` (after Keep / own save). Such files are still walked, watched and remembered by their stamp, so an agent change is listed Modified "no earlier version", Keep only. vr-ide's state dir stays in `Config::excluded` (fully skipped, as before). Test seams `seam::home`, `seam::other_device` (a second device cannot be mounted in a test).
- Interpretation: "unwalked" for other-device folders is implemented as "walked and watched, never copied" — not walking them at all would make their changes silently missing, against SEC-13 / the FR-2 rule.
- Not changed: a file the agent *created* in such a place keeps Revert = move to trash (no pre-image needed); that is the held S2 question.
- Green: 1 passed.
### b. create_store vs sweep
- Red: stress test `sec10_a_store_being_made_survives_a_concurrent_sweep` passed on old code (window too small); deterministic `sec10_a_sweep_between_mkdir_and_lock_leaves_the_new_store_alone` (seam `before_store_lock` runs sweep between lock-file creation and flock) -> `the store was swept: left None, right Some(ino)`.
- Change: store made as `.new-<name>`, lock created + flocked there, then `renameat2(NOREPLACE)` (plain rename fallback on EINVAL/ENOSYS) to `<name>`; a failure removes the `.new-` folder. `sweep` skips `.new-*` younger than 60 s and holds the flock through `remove_dir_all`.
- Green: both pass.
### c. clean_dirs skips excluded folders (SEC-16)
- Red: `sec16_the_ignore_pass_does_not_list_excluded_folders` -> listed `.local/state/vr-ide/accounts/...`.
- Change: `clean_dirs` takes the canonical `State::excluded` (same list as `skipped`) and prunes it in `filter_entry`.
- Green.
### d. row counts after a refused Revert/Keep
- Red: `keep_is_refused…`, `revert_is_refused…` -> row `(9,3)` vs diff `(10,3)`.
- Change: `Changes::diff` writes the diff's counts back into the record (`counts_shown`; full `counts()` when the diff was cut at DIFF_LINES_MAX, None for binary) and bumps the version; `refresh` already runs in the same frame in keep_cmd / answer_revert, so review.rs needed no change.
- Green.

## Commands
- related: `INSTA_UPDATE=no cargo test -p vr-ide --lib -- ai changes` -> 185 passed, 1 failed (the stale golden) -> golden accepted -> green.
- whole crate: `INSTA_UPDATE=no run-heavy.sh cargo test -p vr-ide` -> `929 passed; 0 failed; 7 ignored` (+ 3 passed integration).
- guard: `cargo fmt --all --check` clean; `cargo clippy -p vr-ide --all-targets -- -D warnings` -> Finished, no warnings.

## Reuse
| need | reused |
|---|---|
| no-copy baseline | existing "remembered without a copy" path (non-clean folders) |
| excluded list | `State::excluded` (canonical) |
| test seams | existing `seam` thread-local pattern |

## Rule compliance
| rule | status | evidence |
|---|---|---|
| D27 | pass | item a test |
| SEC-10 | pass | sweep/create tests, private store tests unchanged |
| SEC-11 | pass | no change to link handling; suite green |
| SEC-13 | pass | secret/other-device changes listed, not missing |
| SEC-16 | pass | item c test |
| BASE red-first | pass | red outputs above (b: stress test not red, deterministic seam test red) |
| BASE-11 | pass | only listed files + 2 goldens |
| BASE-24 | pass | |
| BASE-25 | pass | |
| BASE-38 | pass | rustfmt on changed files only; fmt --check clean |
| COST | pass | related + one crate run via run-heavy |
| COST-8 | pass | Edit/perl targeted; diff hunks reviewed |
Nothing left running.

## Review — 2026-11-27

Verdict: **CHANGES_REQUESTED** (1 major code finding; security: ISSUES_FOUND, low only).

### Findings
[major] crates/vr-ide/src/ai/changes.rs:708 — D27 name patterns are matched on every path component, so whole source folders lose Revert
Scenario: the agent edits `src/credentials/provider.py`, `auth/id_tokens/mod.rs`, `src/id_map.rs` or `lib/keys.key/x` → the file is remembered without a copy → the row says "changed, no earlier version kept" (the SEC-9 wording) and Revert is unavailable. Nothing tells the user that the reason is a secret-name rule. D27 lists *name* patterns (`id_*`, `*.pem`, ...) for secret files. Applying them to folders ("as a folder holding files") goes beyond D27, and `id_*` alone already matches ordinary source files.
Fix: match `secret_name` on `rel.file_name()` only (the fixed secret folders already cover `~/.ssh` and the others). Narrowing `id_*` to SSH key names (`id_rsa`, `id_dsa`, `id_ecdsa*`, `id_ed25519*`, optionally `.pub`) is the leader's or user's call against D27. Cost: about one line plus one test case for a source folder named `credentials/`.

[low] crates/vr-ide/src/ai/changes.rs:941-968 — CWE-200: secret folders are matched by absolute path. Canonicalization happens only when the folder exists at start, which leaves bypasses
Data path: `$HOME` that is not canonical (a symlinked `/home`, an automount) + `~/.config/gh` missing at start → `secret` keeps `/home/u/.config/gh`, while `abs` = canonical root + rel = `/var/home/u/.config/gh/hosts.yml` → `no_copy` false.
Exploit: the agent runs `gh auth login` (Created) → the user presses Keep → `rebase` stores the token in the store. The same happens for a relative `$CLAUDE_CONFIG_DIR` (dropped by `is_absolute`), for `$XDG_CONFIG_HOME/gh` / `$GH_CONFIG_DIR`, and for copies of these folders elsewhere (`~/dotfiles/.config/gh/hosts.yml`, `~/backup/.gnupg/secring.gpg`).
Impact: a token copied into the 0700 store (SEC-10 still holds), so the impact is low.
Fix: canonicalize `$HOME` before `join` (and the nearest existing ancestor of each entry), and also match the folder names as relative component sequences (`.ssh`, `.gnupg`, `.aws`, `.config/gh`, `.claude`, `.codex`) anywhere in `rel`.

[minor] crates/vr-ide/src/ai/changes.rs:2766 — `counts_shown` can put older counts back
Scenario: `diff()` reads v1. `recount` then stores v2 counts and clears `recount`. After that `counts_shown` writes v1's counts → the row shows v1's numbers for v2 until the next write. The diff tab also shows v1, so this is not a stale value under the user's eyes.
Fix: skip the write when `r.recount` is set, or compare the stamp read in `diff()` with the file's current stamp under the lock.

[minor] crates/vr-ide/src/ai/changes.rs:2753 — a cut diff (>20 000 lines) runs a second full `counts()` on the UI thread (up to a further 300 ms of DIFF_TIMEOUT)
Fix: when the diff is cut, leave the record's counts as they are (`recount` already wrote full counts) and return.

[minor] crates/vr-ide/src/ai/changes.rs:1050 — `create_store` returns `NotFound` when an old `.new-` folder was swept under it (the process stopped for more than 60 s between creating the lock and flocking it, or the clock jumped) → `Changes::build` fails for that session
Fix: treat `NotFound` like `AlreadyExists` (retry under a new name).

[minor] BASE-25 — the notes ran `cargo clippy -p vr-ide`, not the profile's `--workspace` guard. In practice it is the same here (only vr-ide changed), but the notes should show the exact guard command.

Also noted, not filed: on other devices, files under a non-samefs overlayfs without xino report a per-layer `st_dev` (containers) → everything is no-copy there. This is uncertain and depends on the environment.

### Files read
- crates/vr-ide/src/ai/changes.rs: read
- crates/vr-ide/src/ai/changes_tests.rs: read
- crates/vr-ide/src/ai_changes_tests.rs: read
- …ai_revert_created_80x24.snap, …ai_revert_refused_80x24.snap: read (the row `+9 -3` becomes `+10 -3`, which now matches the diff header already in the old golden)

## Rule check FR-2
| rule | result | evidence |
|---|---|---|
| D27 | fail (major + low above) | `no_copy` is guarded at both `put` sites (1403, 1469). `.ENV`, nested `credentials.json` and `.netrc` are covered; symlinked secret folders are canonicalized when they exist. The folder-component extension and the HOME / relative-env bypasses are filed above |
| D27 "other device = walked + watched, never copied" | pass | keeps the changes listed; D27 itself says changes there are "no earlier version". Leader to confirm the reading |
| SEC-10 | pass | `.new-` dir 0700; lock 0600; rename NOREPLACE with fallback; sweep skips young `.new-*` and holds the flock (`_held`) through `remove_dir_all`; crashed `.new-` swept after 60 s |
| SEC-11 | pass | no change to link and read handling (`read_regular`, `symlink_metadata`) |
| SEC-13 | pass | secret and other-device changes stay listed (d27 test) |
| SEC-16 | pass | `clean_dirs` `filter_entry` prunes the canonical `excluded`; root is canonical |
| counts under lock + version bump | pass (minor race above) | `counts_shown` holds the lock, checks kind/pre, bumps only on change; no reload loop (`open_diff` only runs on user action) |
| test seams not in release | pass | `#[cfg(test)] pub mod seam`; all call sites `#[cfg(test)]` |
| BASE red-first | pass | red outputs in notes; stress test not red, deterministic seam test red |
| BASE-25 | pass (minor note) | fmt --check + clippy -D warnings, narrowed to `-p vr-ide` |
| BASE-11 | pass | only listed files + 2 goldens |
- fix round 1 started (D29, review findings 1-6)
- fix round 1: 6 red tests, fixes applied, related 192 green; running crate + guard

## Fix round 1 — 2026-11-27
Files: crates/vr-ide/src/ai/changes.rs, crates/vr-ide/src/ai/changes_tests.rs (6 new tests). No new goldens.

| # | finding | red (before fix) | change | green |
|---|---|---|---|---|
| 1 | [major] patterns on every component | `d29_source_files_with_secret_like_folder_names_keep_revert`: `the three source files left: 0 right: 3` | D29: `secret_name` on `rel.file_name()` only; `id_*` -> `id_rsa*`, `id_dsa*`, `id_ecdsa*`, `id_ed25519*`; `.netrc` / `.claude.json` also as file names (home reached through a link) | pass |
| 2 | [low] absolute-only secret folders | `d29_secret_folders_by_linked_home_late_creation_and_anywhere` (copies stored for `dotfiles/.config/gh/hosts.yml`, `backup/.gnupg/secring.gpg`); `d29_secret_folders_follow_the_environment`: `real/.ssh not in [.../home/.ssh, ...]` | `resolve()` canonicalizes the longest existing beginning of each path (HOME first, then each entry), so a folder made later under a linked HOME matches; relative env values are joined to the current folder; added `$GH_CONFIG_DIR`, `$XDG_CONFIG_HOME/gh`; `in_secret_folder` matches `.ssh`, `.gnupg`, `.aws`, `.config/gh`, `.claude`, `.codex` as folder sequences anywhere in `rel` | pass |
| 3 | [minor] counts_shown over a newer recount | `a_diff_does_not_put_back_older_counts`: `the newer count stays left: Some(1) right: Some(3)` | skip when `r.recount` is pending, or when the file's stamp under the lock is no longer the diff's `seen` stamp | pass |
| 4 | [minor] second count on a cut diff | `a_cut_diff_does_not_count_again`: `counted again left: 2 right: 1` | cut diff returns early; the recount's full counts stay | pass |
| 5 | [minor] NotFound from a swept `.new-` | `sec10_a_store_swept_while_being_made_is_made_again` panicked at `expect("made under another name")` | NotFound is retried under a new name, like AlreadyExists | pass |
| 6 | BASE-25 exact guard | - | ran the profile guard exactly (below) | pass |

New test-only hooks: `seam::before_counts`, `seam::full_counts` (counts() calls on this thread), `pub(crate) secret_dirs_with(get, cwd)` (environment injected).

Commands:
- related `INSTA_UPDATE=no cargo test -p vr-ide --lib -- ai changes` -> `test result: ok. 192 passed; 0 failed`
- whole crate `INSTA_UPDATE=no $TEAM_BIN/run-heavy.sh cargo test -p vr-ide` -> `test result: ok. 935 passed; 0 failed; 7 ignored` (+3 passed)
- guard `cargo fmt --all --check && $TEAM_BIN/run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` -> fmt clean, clippy `Finished`, no warning/error lines
- log: .ai-dev-team/workspace/qa-artifacts/FR-2/crate-r1.log

Rule compliance (round 1): D27 pass, D29 pass, SEC-10 pass, SEC-13 pass, SEC-16 pass, BASE red-first pass (6/6 red shown), BASE-11 pass, BASE-25 pass (exact guard), BASE-38 pass, COST pass, COST-8 pass — 11 pass / 0 n-a.
Nothing left running.

## Review re-check r1 — 2026-11-27

Verdict: **CHANGES_REQUESTED** on one item that only D29 or the user can settle. The round fixes every earlier finding. If the leader writes the cost below into D29 for the user's confirm-on-return, the verdict becomes APPROVED.

### Earlier findings
- [major] patterns on every component: **fixed**. `secret_name` now runs on `rel.file_name()` only, and `id_*` is narrowed to `id_rsa*`, `id_dsa*`, `id_ecdsa*` and `id_ed25519*`.
- [low] absolute-only secret folders: **fixed**.
  - `resolve()` makes the longest existing beginning of a path canonical: first `$HOME`, then each entry.
  - Relative env values are joined to the current folder.
  - `$GH_CONFIG_DIR` and `$XDG_CONFIG_HOME/gh` were added.
  - `in_secret_folder` matches the secret folder sequences anywhere in the parent of `rel`.
- [minor] counts_shown over a newer recount: **fixed**. A pending `recount`, or a stamp under the lock that differs from `seen`, now skips the write. `seen` is taken before the read, so any write after it is caught.
- [minor] second count on a cut diff: **fixed** (early return).
- [minor] NotFound from a swept `.new-`: **fixed**. A missing base still fails at `create` and does not loop, and the loop stays capped at 16 tries.
- BASE-25: **fixed**. The notes show the exact profile guard command.

### New finding (from D29, for the user to decide)
[major] crates/vr-ide/src/ai/changes.rs:945-952 (`SECRET_SEQS` `.claude`, `.codex`): a project's own `.claude/` / `.codex/` folder loses Revert.
Scenario: a project has a `.claude/` folder, as this repo does. Claude Code edits `.claude/settings.json`, `.claude/settings.local.json` or `.claude/commands/x.md` there. The row then says "changed, no earlier version kept", and the user can only Keep the change. These are among the files agents edit most often, and the files are usually not secrets.
Options:
- (a) Keep as is. Cost: no Revert for any `.claude/` or `.codex/` file in a project.
- (b) Drop `.claude` and `.codex` from `SECRET_SEQS`. The absolute `~/.claude` / `~/.codex` entries, already resolved, stay in place. The name rules still catch `.credentials.json` through `credentials`. Add `auth.json` to the names, only when the parent folder is `.codex`, so a `dotfiles/.codex/auth.json` copy is still caught. This costs about 3 lines and 1 test.

Test-only hooks (`before_counts`, `full_counts`, `counted`, `counts_now`) live inside the `#[cfg(test)] mod seam`, and every call site is `#[cfg(test)]`. `secret_dirs_with` is release code and has no seam.

### Files read
- crates/vr-ide/src/ai/changes.rs: read (secret helpers, `no_copy`, `counts_shown`, `create_store` retry, seams)
- crates/vr-ide/src/ai/changes_tests.rs: skipped line by line. The notes give the red output of each of the 6 tests, and the crate run passed.

## Rule check FR-2 (r1)
| rule | result | evidence |
|---|---|---|
| D27 / D29 | pass (D29 cost above to confirm) | name patterns are checked on the file name; folder sequences and the resolved absolute folders both apply; both `put` sites are guarded |
| SEC-10 | pass | retry on NotFound; `.new-` and sweep logic unchanged |
| SEC-11 | pass | link and read handling unchanged |
| SEC-13 | pass | no-copy files stay listed |
| SEC-16 | pass | unchanged since round 0 |
| counts under lock + bump | pass | recount and stamp checked under the lock |
| test seams not in release | pass | `#[cfg(test)]` |
| BASE red-first | pass | 6 red outputs in the notes |
| BASE-25 | pass | exact guard command |
| BASE-11 | pass | only changes.rs and changes_tests.rs changed this round |
