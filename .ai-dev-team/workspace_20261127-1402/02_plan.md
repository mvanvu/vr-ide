# vr-ide M1–M3 Implementation Plan

> **For agentic workers:** executed by the ai-dev-team build loop (one developer per task, reviewer + qc per task).
> Steps are TDD: failing test → run red → implement → run green. Commits are made by the leader, never by agents.

**Goal:** a lightweight non-modal terminal IDE `vr-ide` that opens the current directory as a project, edits files,
browses/manages the tree, and searches/replaces in a file and across the project (spec milestones M1–M3).

**Architecture:** one binary, cargo workspace of small crates. `vr-core` (text, edits, undo, document I/O) and the
other library crates are UI-free; `vr-ide` owns the event loop, layout, widgets and the command registry. Background
work (file watching, project search, tree loading) runs on threads and talks to the app through `std::sync::mpsc`
channels drained once per frame; the UI thread never blocks on I/O.

**Tech Stack:** Rust stable 1.100.0, edition 2024; ratatui 0.30.2 (+ crossterm 0.29.0), ropey 1.6.1,
unicode-segmentation 1.13.3, unicode-width 0.2.3, arboard 3.7.0, notify 9.0.0 + notify-debouncer-full 0.8.0,
trash 5.4.0, ignore 0.4.35, grep-searcher 0.1.18, grep-regex 0.1.15, grep-matcher 0.1.10, nucleo-matcher 0.3.1,
regex 1.13.2, toml 1.2.2, serde 1.0.229, thiserror 2.0.21, anyhow 1.0.105, tracing 0.1.44, directories 6.0.0;
dev: insta 1.49.0, tempfile 3.27.0, similar 3.2.1.

**Spec:** `.ai-dev-team/workspace/01_spec.md` (D1–D13, C1–C7, AC-1…AC-18).

## Global Constraints
- Non-modal: typing inserts text (D1). Every action is a named command; keys come only from keymap TOML (D2, D10).
- Built-in default keymap = D10 list exactly; user file `~/.config/vr-ide/keymap.toml` overrides per key.
- Project root: `vr-ide` → cwd; `vr-ide <dir>` → dir; `vr-ide <file>` → enclosing git root, else the file's parent (D5, AC-2).
- Delete in explorer → system trash after confirmation (D9).
- Unknown command names in keymap are reported in a startup notice, never a crash (AC-5).
- Targets (C3): first frame < 100 ms, idle RSS < 60 MB, keystroke→frame < 16 ms; measured by `scripts/bench-startup.sh`.
- Panic hook restores the terminal and writes modified buffers to `~/.local/state/vr-ide/recovery/` (C6).
- Library crates use `thiserror`; only `vr-ide` uses `anyhow`. No `unwrap()` on I/O paths.
- Code, comments, docs in English.

## Review Focus
1. Cursor math on multi-codepoint graphemes and wide chars (Vietnamese combining marks, CJK, emoji ZWJ): cursor moves
   one grapheme, column = display width. Pinned in T2 (`grapheme_motion_*`) and T5 (`render_wide_chars` snapshot).
2. Save must never corrupt: CRLF kept, missing trailing newline kept, non-UTF-8 opened read-only, write is atomic
   (temp + rename, permissions kept). Pinned in T4 (`save_*`).
3. External change while modified: AI agents rewrite files under the editor; unmodified → reload, modified → prompt,
   file deleted → buffer kept, marked "deleted on disk". Pinned in T4 (`watch_*`) and T6.
4. Huge trees (node_modules, vendor) and huge files (100 MB): lazy tree expansion, search streams, rendering only the
   visible lines. Pinned in T7 (`tree_lazy_children`), T9 (`project_search_streams`) and T5 (`render_large_file_fast`).
5. Regex replace with captures and zero-length matches is one undo step and does not loop. Pinned in T9 (`replace_all_*`).

---

## File structure
```
Cargo.toml                    workspace, [workspace.dependencies] with every crate version above (T1 only)
rust-toolchain.toml           channel = "stable"
scripts/bench-startup.sh      startup + RSS measurement
crates/vr-core/src/           lib.rs, text.rs (Rope wrapper, grapheme/width), selection.rs, transaction.rs,
                              history.rs, edit_ops.rs (line ops), document.rs (load/save/encoding), watch.rs
crates/vr-config/src/         lib.rs, keys.rs (KeyChord parse/display), keymap.rs, settings.rs, defaults/keymap.toml,
                              defaults/settings.toml, paths.rs
crates/vr-fs/src/             lib.rs, tree.rs, ops.rs, file_index.rs (fuzzy go-to-file)
crates/vr-search/src/         lib.rs, find.rs (in-buffer), project.rs (search), replace.rs (project replace)
crates/vr-ide/src/            main.rs, cli.rs, app.rs, event_loop.rs, commands/ (registry.rs + one file per group),
                              ui/ (layout.rs, editor_view.rs, tabs.rs, status_bar.rs, palette.rs, dialog.rs,
                              explorer.rs, find_bar.rs, search_panel.rs), clipboard.rs, recovery.rs, theme.rs
```
Integration points: the workspace `Cargo.toml`/`Cargo.lock` are written once by T1 with every dependency of M1–M3 so no
later task edits them. `crates/vr-ide/src/commands/registry.rs` and `app.rs` are edited by T5, T6, T8, T10 → those
tasks run in sequence (T5 → T6 → T8 → T10).

## Contracts (crate APIs later tasks rely on)

### vr-core (T2, T4)
```rust
pub struct Pos { pub line: usize, pub col: usize }            // col = grapheme index within the line
pub struct Range { pub anchor: usize, pub head: usize }       // char offsets into the rope; head = cursor
pub struct Selection { ranges: Vec<Range>, primary: usize }   // single range in M1 UI, many for batch edits
pub struct Change { pub from: usize, pub to: usize, pub insert: String }   // char offsets, non-overlapping, sorted
pub struct Transaction { pub changes: Vec<Change>, pub selection_after: Option<Selection> }
pub struct Buffer { /* Rope, Selection, History, version: u64 */ }
impl Buffer {
  pub fn from_str(s: &str) -> Self; pub fn text(&self) -> &ropey::Rope; pub fn version(&self) -> u64;
  pub fn selection(&self) -> &Selection; pub fn set_selection(&mut self, s: Selection);
  pub fn apply(&mut self, tx: Transaction);                     // one undo group
  pub fn undo(&mut self) -> bool; pub fn redo(&mut self) -> bool;
  pub fn insert_text(&mut self, s: &str); pub fn delete_backward(&mut self); pub fn delete_forward(&mut self);
}
pub mod motion { move_left/right/up/down/word_left/word_right/line_start/line_end/doc_start/doc_end(buf, extend: bool) }
pub mod edit_ops { delete_line, duplicate_line, move_line_up, move_line_down, indent, outdent,
                   toggle_comment(buf, token: &str) }           // each = one Transaction
pub fn display_col(line: ropey::RopeSlice, char_idx: usize, tab_width: usize) -> usize;
pub enum LineEnding { Lf, CrLf }  pub enum Encoding { Utf8, Utf8Bom, Other(String) }
pub struct Document { pub path: Option<PathBuf>, pub buffer: Buffer, pub line_ending: LineEnding,
                      pub encoding: Encoding, pub read_only: bool, saved_version: u64, pub disk_state: DiskState }
pub enum DiskState { InSync, ChangedOnDisk, DeletedOnDisk }
impl Document { pub fn open(path: &Path, max_size_highlight: u64) -> Result<Self, DocError>;
                pub fn is_modified(&self) -> bool; pub fn save(&mut self) -> Result<(), DocError>;
                pub fn save_as(&mut self, p: &Path) -> Result<(), DocError>; pub fn reload(&mut self) -> Result<(), DocError>; }
pub enum DocError { Io(io::Error), Binary, PermissionDenied(PathBuf), NotUtf8ReadOnly }
pub struct Watcher; impl Watcher { pub fn new(tx: Sender<FsEvent>) -> Result<Self>; pub fn watch(&mut self, p: &Path); pub fn unwatch(..) }
pub enum FsEvent { Modified(PathBuf), Removed(PathBuf), Created(PathBuf), Renamed(PathBuf, PathBuf) }
```

### vr-config (T3)
```rust
pub struct KeyChord { pub code: KeyCode, pub mods: Mods }      // own types, converted from crossterm in vr-ide
impl FromStr for KeyChord   // "ctrl+shift+f", "alt+up", "ctrl+pagedown", "f12", "ctrl+/"
impl Display for KeyChord
pub struct Keymap { /* KeyChord -> command name */ }
impl Keymap { pub fn load(user: Option<&Path>) -> (Keymap, Vec<ConfigWarning>);  // defaults + user overrides
              pub fn command_for(&self, k: &KeyChord) -> Option<&str>; pub fn keys_for(&self, cmd: &str) -> Vec<KeyChord>;
              pub fn validate(&self, known: &[&str]) -> Vec<ConfigWarning>; }  // unknown command names
pub struct Settings { pub tab_width: usize, pub insert_spaces: bool, pub theme: String, pub show_hidden: bool,
                      pub show_ignored: bool, pub max_highlight_bytes: u64, pub scrollback: usize,
                      pub ai_presets: Vec<AiPreset> }  // ai_presets parsed now, used in M6
pub fn config_dir() -> PathBuf;  pub fn state_dir() -> PathBuf;   // ~/.config/vr-ide, ~/.local/state/vr-ide
```

### vr-fs (T7)
```rust
pub struct Tree { pub root: PathBuf }  pub struct Node { pub path: PathBuf, pub kind: NodeKind, pub ignored: bool, pub depth: usize }
impl Tree { pub fn new(root, show_hidden, show_ignored) -> Self; pub fn children(&self, dir: &Path) -> io::Result<Vec<Node>>; }  // lazy, dirs first, case-insensitive sort, symlink loops safe
pub mod ops { create_file, create_dir, rename, copy(recursive), move_to, trash(paths) -> Result<(), FsOpError> }
pub struct FileIndex; impl FileIndex { pub fn build(root, tx: Sender<IndexMsg>) -> JoinHandle<()>;   // background, ignore-aware
                                      pub fn query(&self, q: &str, limit: usize) -> Vec<(PathBuf, u32 score, Vec<u32> indices)>; }
```

### vr-search (T9)
```rust
pub struct Query { pub pattern: String, pub regex: bool, pub case_sensitive: bool, pub whole_word: bool }
pub fn find_all(text: &ropey::Rope, q: &Query) -> Result<Vec<(usize, usize)>, QueryError>;   // char ranges
pub fn replace_all_tx(text: &Rope, q: &Query, replacement: &str) -> Result<Transaction, QueryError>;  // $1 captures
pub struct ProjectMatch { pub path: PathBuf, pub line: usize, pub col: usize, pub line_text: String, pub match_range: (usize, usize) }
pub fn search_project(root, q, cancel: Arc<AtomicBool>, tx: Sender<SearchMsg>) -> JoinHandle<()>;   // streams batches
pub enum SearchMsg { Batch(Vec<ProjectMatch>), Done { files: usize, matches: usize, elapsed: Duration }, Error(String) }
pub fn plan_replace(root, q, replacement, files: &[PathBuf]) -> Vec<FilePreview>;   // before/after hunks per file
pub fn apply_replace(previews: &[FilePreview], confirmed: &[PathBuf]) -> Vec<(PathBuf, Result<(), io::Error>)>;
```
Open buffers: project replace on a file that is open in a tab is applied to the buffer as one Transaction (undoable,
AC-12), not written behind it; `vr-ide` routes those files to the buffer and the rest to `apply_replace`.

### vr-ide command registry (T5, extended by T6/T8/T10)
```rust
pub struct CommandSpec { pub name: &'static str, pub title: &'static str, pub run: fn(&mut App, CmdArgs) -> CmdResult }
pub fn all_commands() -> &'static [CommandSpec];   // single list, grouped files append their slice
```
Command names (stable, used by keymap): `app.quit, app.palette, file.save, file.save_as, file.close, file.open,
tab.next, tab.prev, edit.undo, edit.redo, edit.copy, edit.cut, edit.paste, edit.select_all, edit.delete_line,
edit.duplicate_line, edit.move_line_up, edit.move_line_down, edit.indent, edit.outdent, edit.toggle_comment,
nav.goto_line, file.reload, explorer.toggle, explorer.focus, explorer.reveal, explorer.new_file, explorer.new_folder,
explorer.rename, explorer.copy, explorer.paste, explorer.move, explorer.delete, explorer.toggle_ignored,
explorer.toggle_hidden, file.goto (fuzzy), find.open, find.next, find.prev, find.replace_open, find.replace_one,
find.replace_all, search.project, search.next_result, search.prev_result, search.replace_project, focus.cycle`.
Cursor movement and typing are built-in editor handling (arrows, home/end, pgup/pgdn, shift+ to extend, ctrl+left/right
word, backspace/delete/enter/tab) and not rebindable in M1.

## Decisions
| Decision | Options | Choice | Why |
|---|---|---|---|
| Rope | ropey 1.6.1 / ropey 2.0 beta / own piece table | ropey 1.6.1 | latest stable; 2.0 is a pre-release (TOOL-2) |
| Offsets | byte / char / grapheme | char offsets in core, grapheme for motions, display width for columns | ropey is char-indexed; graphemes for correct cursor; width for rendering |
| Async | tokio / threads + mpsc | threads + mpsc | lighter binary and RSS; nothing in M1–M3 needs an async runtime (LSP in M8 may revisit) |
| Clipboard | arboard / OSC 52 / both | arboard, OSC 52 fallback when no display (SSH) | AC-4 |
| Save | write in place / temp + rename | temp in same dir + fsync + rename, keep mode | crash safety |
| Fuzzy | nucleo-matcher / skim / own | nucleo-matcher | fast, used by Helix |
| Rendering tests | snapshots / manual | ratatui TestBackend + insta | deterministic UI regression tests |
| Encoding | full encoding_rs / UTF-8 only | UTF-8 (+BOM) editable; other → read-only notice | spec edge case; avoids corruption, keeps deps small |

## Toolchain
| Tool | Version | Checked via | Date |
|---|---|---|---|
| rustc / cargo | 1.100.0 stable | `rustc --version` | 2026-11-24 |
| crates | as listed in Tech Stack | `cargo search`, `cargo info` | 2026-11-24 |

## Risks & mitigation
- Terminal key reporting: crossterm cannot see `ctrl+shift+<letter>` on terminals without the kitty keyboard protocol →
  T5 enables `PushKeyboardEnhancementFlags` when supported, and the palette always works; the notice lists unreachable keys.
- `ctrl+c` / `ctrl+q` / `ctrl+s` swallowed by the tty (XON/XOFF, SIGINT) → raw mode disables them; T5 tests the mapping.
- Watcher storms during `git checkout` or agent bulk edits → debounce 200 ms, coalesce per path.

---

## Tasks

### Task T1: Workspace scaffold (TOOL)
**Files:** Create `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.gitignore` (target/, `.remember/`), `rustfmt.toml`,
`clippy.toml` (optional), `crates/{vr-core,vr-config,vr-fs,vr-search,vr-ide}/Cargo.toml` + `src/lib.rs`/`src/main.rs`
stubs, `scripts/bench-startup.sh`, `README.md` (build/install/run).
**Produces:** a workspace where `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace
--all-targets -- -D warnings` and `cargo fmt --all --check` pass; `[workspace.dependencies]` holds every crate of the
Tech Stack at the listed version; each crate's `Cargo.toml` already lists the workspace deps it will use per the
contracts above (so later tasks never edit manifests). `vr-ide` binary: enters raw mode + alternate screen, draws
"vr-ide — <root>" and quits on ctrl+q; a panic hook restores the terminal.
- [ ] Test `cli_resolves_root` (in `vr-ide/src/cli.rs`): no arg → cwd; dir arg → canonical dir; file arg inside a temp
      git repo (`git init`) → repo root + file; file arg outside git → parent dir; nonexistent path → `Err` with message.
- [ ] Run red, implement `pub fn resolve(args: &[OsString], cwd: &Path) -> Result<Launch, CliError>` with
      `pub struct Launch { pub root: PathBuf, pub open_file: Option<PathBuf> }`, run green.
- [ ] `scripts/bench-startup.sh`: builds release, runs `vr-ide` under a pty (`script -qc`) with `VR_IDE_EXIT_AFTER_FIRST_FRAME=1`,
      prints time to first frame and max RSS (`/usr/bin/time -v`); the binary honours that env var.
- [ ] Verify all four workspace commands pass; record their real output in the notes.

### Task T2: vr-core text, selection, transactions, history, edit ops
**Files:** `crates/vr-core/src/{lib,text,selection,transaction,history,motion,edit_ops}.rs`, tests in each module.
**Produces:** the vr-core contract above minus `Document`/`Watcher`.
- [ ] Tests: `insert_and_delete_ascii`; `grapheme_motion_combining` ("Tiếng Việt" with combining marks: move_right steps
      one grapheme); `grapheme_motion_emoji_zwj` ("👨‍👩‍👧" is one step); `display_col_wide` ("中a" → col of 'a' = 2;
      tab with width 4); `undo_redo_groups` (typed "abc" as consecutive inserts = one undo group; group breaks on
      cursor jump or after 1 s idle — time injected); `apply_multi_change_one_undo` (3 non-overlapping changes →
      one undo restores original); `selection_maps_through_changes`; `delete_line_last_line_no_newline`;
      `duplicate_line`; `move_line_up_down_bounds`; `indent_outdent_selection` (spaces per settings);
      `toggle_comment_php` (`//` added to all selected lines, removed when all commented, indentation kept);
      `word_motion_unicode`; `crlf_text_treated_as_one_line_break`.
- [ ] Implement per contract; `History` stores inverse transactions; version increments per apply/undo/redo.
- [ ] `cargo test -p vr-core` green; clippy clean.

### Task T3: vr-config keymap and settings
**Files:** `crates/vr-config/src/{lib,keys,keymap,settings,paths}.rs`, `crates/vr-config/defaults/{keymap,settings}.toml`.
**Produces:** the vr-config contract. `defaults/keymap.toml` = D10 list mapped to the command names above, plus
`alt+q = "focus.editor"` reserved for M5 (D8) — documented as reserved.
- [ ] Tests: `parse_chords` (table of strings incl. "ctrl+shift+f", "alt+up", "ctrl+/", "ctrl+`", "f12", invalid "ctrl+" → Err);
      `display_roundtrip`; `defaults_load_without_user_file`; `user_overrides_single_key` (user binds ctrl+s to
      "file.save_as" → only that changes); `user_unbinds_with_empty_string`; `unknown_command_reported`
      (`validate` returns warning naming key + command, no panic); `malformed_toml_reported_defaults_kept`;
      `settings_defaults_and_override`; `keys_for_lists_all_bindings`.
- [ ] Implement; `cargo test -p vr-config` green; clippy clean.

### Task T4: vr-core documents and file watching
**Depends:** T2. **Files:** `crates/vr-core/src/{document,watch}.rs` (+ `lib.rs` exports).
- [ ] Tests (tempfile): `open_utf8_lf`; `save_keeps_crlf`; `save_keeps_missing_trailing_newline`; `save_keeps_bom`;
      `open_non_utf8_read_only` (Latin-1 bytes → `read_only = true`, save → `Err(NotUtf8ReadOnly)`); `open_binary_refused`
      (NUL in first 8 KiB → `Err(Binary)`); `save_atomic_keeps_mode` (0o755 kept, no temp file left);
      `save_permission_denied` → `Err(PermissionDenied)`; `is_modified_tracks_saved_version` (edit + undo back = not modified);
      `reload_replaces_text_keeps_cursor_clamped`; `open_100mb_under_2s` (`#[ignore]`, run at sign-off);
      `watch_modified_event` (write file → `FsEvent::Modified` within 1 s); `watch_removed_event`; `watch_debounces_burst`
      (20 writes in 100 ms → ≤ 2 events).
- [ ] Implement; `cargo test -p vr-core` green.

### Task T5: App shell, editor view, commands, clipboard (M1 UI part 1)
**Depends:** T1, T2, T3, T4. **Files:** `crates/vr-ide/src/{main,app,event_loop,clipboard,recovery,theme}.rs`,
`commands/{registry,app,file,edit,tab}.rs`, `ui/{layout,editor_view,tabs,status_bar}.rs`, snapshot files under
`crates/vr-ide/src/ui/snapshots/`.
**Produces:** `App` state (`docs: Vec<Document>`, `active: usize`, `focus: Focus`, `notices: VecDeque<Notice>`,
`keymap`, `settings`, `fs_rx`), event loop (crossterm events + `FsEvent` channel, redraw only when dirty),
key → command dispatch via `Keymap`, built-in editing keys, mouse click/drag selection and wheel scroll,
tabs with modified marker `●`, status bar (`file · Ln x, Col y · LF/CRLF · UTF-8 · notices`), clipboard
(arboard, OSC 52 fallback), panic hook → recovery files, keyboard enhancement flags when supported,
startup notice for config warnings (AC-5).
- [ ] Tests: `dispatch_ctrl_s_saves` (AC-5); `unknown_command_in_keymap_shows_notice_not_crash`;
      `typing_marks_modified_and_save_clears` (AC-3); `undo_redo_via_commands`;
      `copy_paste_uses_clipboard` (clipboard trait with an in-memory fake; AC-4); `cut_removes_selection`;
      snapshots at 80x24 and 40x12: `render_empty_project`, `render_file_with_selection`, `render_wide_chars`,
      `render_tabs_modified`, `render_long_line_horizontal_scroll`; `render_large_file_fast` (100k-line rope renders a
      frame in < 5 ms in release, `#[ignore]` for debug); `panic_hook_writes_recovery` (calls the hook fn directly);
      `external_modify_unmodified_doc_reloads` (AC-7, feed `FsEvent::Modified`).
- [ ] Implement; `cargo test -p vr-ide`; manual smoke under a pty recorded in notes (open file, type, save, quit).

### Task T6: Command palette, dialogs, go-to-line, external-change prompt (M1 UI part 2)
**Depends:** T5. **Files:** `ui/{palette,dialog}.rs`, `commands/{nav,palette}.rs`, `app.rs` (dialog state), snapshots.
- [ ] Tests: `palette_filters_by_fuzzy_name_and_shows_keys` (AC-6: typing "sav" lists file.save with "ctrl+s");
      `palette_enter_runs_command`; `palette_escape_closes`; `goto_line_moves_cursor_and_clamps`;
      `modified_doc_changed_on_disk_prompts` (AC-7: dialog with Reload / Keep mine; Reload replaces text, Keep marks
      buffer as diverged and save asks to overwrite); `deleted_on_disk_marks_tab`; `quit_with_unsaved_asks`
      (Save all / Discard / Cancel); `save_as_prompt`; snapshots `render_palette`, `render_confirm_dialog`.
- [ ] Implement; tests green. **M1 usable build at the end of T6.**

### Task T7: vr-fs tree, file operations, fuzzy file index
**Depends:** T1. **Files:** `crates/vr-fs/src/{lib,tree,ops,file_index}.rs`.
- [ ] Tests (tempfile + `git init` for ignore rules): `tree_dirs_first_sorted`; `tree_hides_gitignored_by_default`
      (AC-9) and shows them flagged when `show_ignored`; `tree_hides_dotfiles_unless_show_hidden` (`.git` always hidden);
      `tree_lazy_children` (only the requested dir is read); `tree_symlink_loop_safe`; `create_file_and_dir`;
      `create_existing_errors`; `rename_file_and_dir`; `copy_dir_recursive`; `move_into_dir`; `move_into_self_refused`;
      `trash_moves_to_trash` (uses `trash` crate; test runs only when `XDG_DATA_HOME` points to a temp dir —
      set it in the test); `file_index_respects_gitignore`; `file_index_query_ranks_basename_first`
      ("ordctl" → `src/Controller/OrderController.php` first).
- [ ] Implement; `cargo test -p vr-fs` green.

### Task T8: Explorer panel and go-to-file (M2 UI)
**Depends:** T6, T7. **Files:** `ui/explorer.rs`, `commands/explorer.rs`, `app.rs`, `ui/layout.rs`, `ui/palette.rs`
(go-to-file mode reuses the palette widget), snapshots.
- [ ] Tests: `explorer_shows_root_tree` (AC-1); `explorer_expand_collapse_keyboard_and_mouse`;
      `explorer_new_file_opens_it`; `explorer_rename_open_file_updates_tab` (AC-8); `explorer_delete_confirms_then_trashes`
      (D9; fake trash in test); `explorer_copy_paste_and_move`; `explorer_reveal_active_file`;
      `explorer_toggle_ignored`; `explorer_refreshes_on_fs_events` (file created outside → appears);
      `goto_file_opens_selected` ; `focus_cycle_editor_explorer`; snapshots `render_explorer_80x24`, `render_explorer_narrow`.
- [ ] Implement; tests green. **M2 usable build at the end of T8.**

### Task T9: vr-search engine (in-buffer find/replace, project search, project replace)
**Depends:** T2 (Transaction), T7 not required. **Files:** `crates/vr-search/src/{lib,find,project,replace}.rs`.
- [ ] Tests: `find_plain_case_insensitive`; `find_whole_word`; `find_regex_multiline_off`; `invalid_regex_is_error`;
      `replace_all_regex_captures` (AC-10: `fn (\w+)` → `fn new_$1`, one Transaction); `replace_all_zero_length_no_loop`
      (`^` → `// `); `replace_all_unicode_offsets` (matches after Vietnamese text land on correct char offsets);
      `project_search_respects_gitignore` (AC-9); `project_search_streams` (first `Batch` arrives before `Done` on a
      2 000-file fixture); `project_search_cancel_stops`; `project_search_skips_binary`;
      `plan_replace_previews_hunks`; `apply_replace_only_confirmed_files` (AC-12); `apply_replace_keeps_crlf`.
- [ ] Implement with `ignore::WalkParallel` + `grep-searcher`; batches of ≤ 200 matches or every 50 ms. Tests green.

### Task T10: Find bar, replace bar, project search panel (M3 UI)
**Depends:** T8, T9. **Files:** `ui/{find_bar,search_panel}.rs`, `commands/{find,search}.rs`, `app.rs`, `ui/layout.rs`, snapshots.
- [ ] Tests: `find_bar_highlights_matches_and_counts` ("3/12"); `find_next_prev_wraps`; `find_toggles_regex_case_word`;
      `replace_one_and_all_single_undo` (AC-10); `project_search_lists_results_grouped_by_file` (AC-11);
      `enter_on_result_opens_file_at_line_col`; `project_replace_preview_and_confirm` (AC-12: unchecked file untouched;
      open file changed via its buffer and undoable); `search_panel_cancel_on_new_query`; snapshots `render_find_bar`,
      `render_search_panel`.
- [ ] Implement; tests green. **M3 usable build at the end of T10.**

### Task T11: Bench, docs, sign-off prep
**Depends:** T10. **Files:** `README.md`, `scripts/bench-startup.sh`, `docs/keymap.md` (generated list of commands + default keys), `crates/vr-ide/src/commands/registry.rs` (only a `--list-commands` flag if needed for docs).
- [ ] Run bench on release build with a 5k-file fixture project; record first-frame ms and RSS against C3 targets.
- [ ] `docs/keymap.md` lists every command name, title and default key; README covers install (`cargo install --path
      crates/vr-ide`), launch (D5), config files and the keymap override example.

## Coverage
AC-1 T1/T8 · AC-2 T1 · AC-3 T2/T5 · AC-4 T5 · AC-5 T3/T5 · AC-6 T6 · AC-7 T4/T5/T6 · AC-8 T7/T8 · AC-9 T7/T9 ·
AC-10 T9/T10 · AC-11 T9/T10 · AC-12 T9/T10 · AC-18 T4/T5. AC-13…AC-17 belong to M4–M8 (not this run).

## Critical path
`T1 → T2 → T4 → T5 → T6 → T8 → T10 → T11 → sign-off`. Parallel: T3 ∥ T2 (after T1); T7 ∥ T2/T4 (after T1);
T9 after T2, beside T4–T8.

## M4–M8 outline (detailed when each starts; keys follow VS Code defaults per D10, no question to the user)
- M4 Highlighting: `vr-syntax` crate, tree-sitter + static grammars (C2), highlight queries, theme scopes, incremental reparse on Transaction.
- M5 Terminal: `vr-term` crate, portable-pty + alacritty_terminal (C1), bottom panel tabs, focus rule D8 (`alt+q`).
- M6 AI console: presets from `settings.ai_presets`, "send selection/path to AI tab" command, relies on T4 watching.
- M7 Git: `vr-git` crate, gix reads + git CLI writes (C5), gutter signs, git panel, diff view, hunk staging.
- M8 LSP: `vr-lsp` crate, JSON-RPC over stdio, completion/hover/definition/references/diagnostics/rename/format/code actions.
