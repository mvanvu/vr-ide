# Spec: vr-ide — a lightweight terminal IDE in Rust
flags: has_ui=true (terminal UI), security_sensitive=false, platforms=linux (macOS compiles, not a v1 target)

## Goal
The user's daily driver for writing code and managing source code, replacing PhpStorm, Neovim and VS Code.
The overriding quality is **lightness**: instant start, low memory, runs entirely in a terminal.
Typing `vr-ide` in a shell opens the IDE on the current directory as the project.

## Users & context
- One user (the owner), a polyglot developer: PHP (Symfony/Laravel), JS/TS, Node/NestJS, Vue, React,
  HTML/CSS/SCSS/SASS, Dart/Flutter, Go, Python, Rust.
- Works daily with AI coding agents in the terminal (Claude Code `claude`, `cursor-agent`, …).
- Linux workstation; the IDE will also be used to develop itself.

## Decisions taken with the user
- D1 Editing style: **new, simple, non-modal**. Typing inserts text. No Vim modes, motions or operators.
- D2 Keybindings: defined **incrementally by the user**. Every action is a named command
  (`file.save`, `search.project`, …); keys map to commands in `~/.config/vr-ide/keymap.toml`.
  When a task adds commands, the leader asks the user which keys to bind before the task starts.
  Unbound commands are always reachable through the command palette.
- D3 Architecture: **approach A** — one Rust binary, cargo workspace of small crates, single process.
- D4 Name: `vr-ide`; the project folder is `vr-ide`; binary `vr-ide`.
- D5 Launch: `vr-ide` opens the current working directory as project root; `vr-ide <path>` opens that
  directory (or that file, with its parent/git root as project).
- D6 AI console: a dedicated pane that runs AI agent CLIs (claude, cursor-agent, …) in the project root;
  the IDE does not call model APIs itself.
- D7 Languages: syntax highlighting + LSP for the list above, declared in `languages.toml`
  (extension → tree-sitter grammar → language server command), so adding a language is config only.

## Approaches considered
- **A. Single binary, everything in-house (chosen)** — lightest, fastest start, full control over the new
  editing style; most code to write, offset by proven crates (ropey, tree-sitter, ignore/grep, gix, portable-pty).
- B. Core daemon + TUI client over RPC (Xi/Neovim style) — enables GUI/remote/agent control later, but more
  complexity, latency and failure modes than a lightweight v1 deserves. `core` is kept UI-free so B stays possible.
- C. Fork Helix — LSP and tree-sitter for free, but modal at its core (contradicts D1) and a large fork to maintain.

## Layout (text sketch)
```
┌ vr-ide — ~/DEV/shop ─────────────────────────────────────────────────────┐
│ EXPLORER        │ src/Controller/Order.php ● │ routes.yaml │              │
│ ▾ src           │  12  public function list(): Response                   │
│   ▾ Controller  │ ~13  {                                                  │
│     Order.php   │ +14      $orders = $this->repo->findAll();              │
│   ▸ Entity      │  15      return $this->json($orders);                   │
│ ▸ tests         │  16  }                                                  │
│   composer.json │                                                         │
├─────────────────┴─────────────────────────────────────────────────────────┤
│ [TERMINAL 1] [TERMINAL 2] [AI: claude] [AI: cursor]                       │
│ $ php bin/console cache:clear                                             │
├───────────────────────────────────────────────────────────────────────────┤
│ main ↑1 ~3 │ PHP │ intelephense ✓ │ E:0 W:2 │ Ln 14, Col 9 │ UTF-8 LF     │
└───────────────────────────────────────────────────────────────────────────┘
```
Panels (explorer, bottom panel, git panel) toggle and resize; the editor area can split vertically/horizontally.

## Scope
### In scope (v1), in delivery order
1. **Core editor** — open/edit/save files; multiple open files as tabs; undo/redo (grouped); selection with
   keyboard and mouse; copy/cut/paste via system clipboard (OSC 52 fallback over SSH); delete line,
   duplicate line, move line, indent/outdent, comment toggle; line numbers; status bar; command palette;
   keymap and settings from TOML; detection of external changes (auto-reload when unmodified, prompt when modified).
2. **File explorer** — gitignore-aware tree; create file/folder, rename, copy, move, delete (to system trash);
   reveal current file; fuzzy "go to file".
3. **Search & replace** — in-file find/replace (plain, regex, case, whole word, next/prev/all);
   project-wide search (ripgrep engine) with result list and jump; project-wide replace with per-file preview.
4. **Syntax highlighting** — tree-sitter for the D7 languages (PHP, Blade/Twig where a grammar exists,
   JS/TS/TSX, Vue, HTML, CSS/SCSS, Dart, Go, Python, Rust, JSON/YAML/TOML/Markdown); theme from config.
5. **Integrated terminal** — PTY-backed terminals in the bottom panel, several tabs, full-screen
   TUIs inside work (colors, cursor, alternate screen), scrollback, starts in project root.
6. **AI console** — terminal tabs launched from presets in config (`claude`, `cursor-agent`, …), started in the
   project root, toggled by one command; a command sends the current selection or file path to the active AI tab.
   Files the agents change are picked up through (1)'s external-change detection.
7. **Git** — gutter signs (added/changed/removed) and branch/ahead-behind in the status bar; a git panel with
   changed files, side-by-side or inline diff, stage/unstage file and hunk, discard (with confirmation), commit,
   amend, branch list/switch/create, pull/push/fetch (via the `git` CLI), log and blame of the current file.
8. **LSP** — completion, hover, go to definition, find references, diagnostics in gutter and list, rename,
   format document, code actions; servers launched per `languages.toml`, missing servers reported, not fatal.

### Out of scope (v1)
GUI; plugins/scripting; Vim mode; debugger (DAP); remote/SSH editing beyond running in an SSH terminal;
project-local config that runs commands (workspace trust); Windows; built-in AI API calls; session restore
(candidate for v1.1).

## Acceptance criteria
- AC-1: Given a shell in `~/DEV/shop`, When the user types `vr-ide`, Then the IDE opens with `~/DEV/shop` as
  project root, the explorer shows its tree and the title shows the path.
- AC-2: Given `vr-ide src/Order.php`, When it starts, Then that file is open and the project root is the enclosing git root.
- AC-3: Given an open file, When the user types, deletes, undoes and redoes, Then the buffer content matches the
  expected text at each step and a modified marker shows until saved; saving writes the file atomically.
- AC-4: Given a selection, When copy then paste elsewhere (also into another app), Then the system clipboard holds the text.
- AC-5: Given `keymap.toml` binds `ctrl+s = "file.save"`, When the user presses ctrl+s, Then the file saves;
  an unknown command name in the keymap is reported at start, not a crash.
- AC-6: Given the command palette, When the user types part of a command name, Then matching commands are listed with their bound keys and Enter runs one.
- AC-7: Given an unmodified open file changed on disk (e.g. by an AI agent), Then the buffer reloads within 1 s;
  given a modified one, Then the user is asked to reload or keep theirs.
- AC-8: Given the explorer, When the user creates, renames, copies, moves and deletes files and folders, Then the
  disk reflects each action, deleted items go to the system trash, and open tabs follow renames.
- AC-9: Given a project with `.gitignore`, Then ignored paths are hidden in the explorer (toggleable) and excluded from go-to-file and project search.
- AC-10: Given in-file find with regex `fn (\w+)` and replace `fn new_$1`, When "replace all", Then every match is replaced in one undo step.
- AC-11: Given a 50k-file project, When the user searches a term, Then the first results appear in < 300 ms and the list streams; Enter jumps to file:line:col.
- AC-12: Given project replace, Then a per-file preview is shown and only confirmed files are written, each undoable while open.
- AC-13: Given a file of each D7 language, Then it is highlighted by its tree-sitter grammar; an unknown extension opens as plain text.
- AC-14: Given the terminal panel, When the user runs `htop`, `lazygit` or `vim` inside, Then it renders correctly and resizes with the panel.
- AC-15: Given AI preset `claude` in config, When the user opens it, Then a terminal tab runs `claude` in the project root;
  when `claude` is not installed, the tab shows a clear "command not found" message.
- AC-16: Given a git repo with edits, Then gutter signs mark changed lines and the git panel lists changed files with diffs;
  stage/unstage of a file and of a hunk, commit and branch switch produce the same repo state as the git CLI would.
- AC-17: Given a PHP file and `intelephense` installed, Then completion, hover, go to definition, references, diagnostics
  and rename work; given the server is missing, Then the status bar says so and editing continues.
- AC-18: Given a 100 MB text file, When opened, Then it opens and scrolls without freezing the UI (highlighting may be disabled above a configurable size).

## Edge cases
Non-UTF-8 files (open read-only or with detected encoding, never corrupt on save); CRLF line endings preserved;
files without trailing newline; very long lines; binary files (refuse or hex notice); permission-denied save;
file deleted on disk while open; symlinks and symlink loops in the tree; huge directories (node_modules, vendor)
loaded lazily; terminal resize to tiny sizes; wide/CJK/emoji characters and Vietnamese diacritics in cursor math;
LSP server crash (restart, no editor crash); detached HEAD, merge conflicts and no-git folders.

## Non-functional constraints
- Start to first frame < 100 ms on a mid-size project; idle RSS < 60 MB without LSP servers.
- Keystroke to screen < 16 ms; no blocking I/O on the UI thread (search, git, LSP, file watch run off-thread).
- Single static-ish binary, installed with `cargo install --path crates/vr-ide`; latest stable Rust.
- Config under `~/.config/vr-ide/` (`settings.toml`, `keymap.toml`, `languages.toml`, `themes/`), with defaults built in.
- Crash safety: a panic restores the terminal and writes unsaved buffers to a recovery folder.

## Leader's calls (after builder check; user may override)
- C1 Terminal emulation: `alacritty_terminal` crate (truecolor, mouse reporting, alt screen, bracketed paste),
  rendered into ratatui; scrollback default 10 000 lines, configurable. No custom VT parser.
- C2 Grammars: the D7 grammars are compiled into the binary (static, one cargo feature per language group);
  `languages.toml` maps extensions/servers to built-in grammars. Runtime-loaded `.so` grammars are v1.1.
  Where no usable grammar exists (e.g. Blade/Twig), the file falls back to the HTML grammar.
- C3 Performance numbers are **targets**, measured by a startup/RSS bench script from M1 on and reported per
  milestone; they become gates only if the user asks.
- C4 Delivery: the 8 scope items are milestones M1…M8, each with its own review/QC and a usable build at the end.
  The plan details M1 fully and outlines M2–M8; each later milestone is detailed when it starts.
  `core` supports batch edits (one undo group, many ranges) from M1, since replace-all, LSP rename and code actions need it.
- C5 Git: `gix` for reading (status, diff, blame, log, branch list); the `git` CLI for writes
  (stage/unstage, hunk staging via `git apply --cached`, commit, branch, pull/push/fetch).
- C6 File watching: `notify` (inotify) with debounce; recovery folder `~/.local/state/vr-ide/recovery/`.
- C7 Before keys are defined, only three defaults exist: `ctrl+p` palette, `ctrl+s` save, `ctrl+q` quit;
  each milestone asks the user for its keys before it starts (D2).

## Left to the builder
Crate choices within approach A, internal module boundaries, test strategy (unit tests on `core`,
snapshot tests of rendered frames with ratatui's `TestBackend`), theme default.

## Decisions from the up-front questions (user, 11:21)
- D8 Terminal focus: a focused terminal / AI tab receives **every key** except one escape key, `alt+q`, which returns focus to the editor.
- D9 Delete in the explorer moves items to the **system trash**, after a confirmation.
- D10 Keymap (user, 11:27 — supersedes D2's "ask per milestone" and C7): the built-in `keymap.toml` follows
  **VS Code's default keybindings (Linux)** for every milestone, M4–M8 included; the user is **not asked** about keys.
  Examples: ctrl+shift+p and f1 command palette, ctrl+p go to file, ctrl+s save, ctrl+shift+s save as, ctrl+n new file,
  ctrl+o open, ctrl+w close tab, ctrl+pagedown/pageup next/prev tab, ctrl+c/x/v copy/cut/paste, ctrl+z undo,
  ctrl+shift+z and ctrl+y redo, ctrl+a select all, ctrl+f find, ctrl+h replace, f3/shift+f3 next/prev match,
  ctrl+shift+f search in files, ctrl+shift+h replace in files, ctrl+g go to line, ctrl+/ toggle comment,
  ctrl+] / ctrl+[ indent/outdent, alt+up/down move line, shift+alt+down/up copy line down/up, ctrl+shift+k delete line,
  ctrl+b toggle sidebar, ctrl+shift+e explorer focus, f2 rename (explorer), delete (explorer delete), ctrl+q quit;
  later: ctrl+` terminal, ctrl+shift+g git, f12 go to definition, shift+f12 references, ctrl+space completion,
  ctrl+shift+i format. Where a terminal cannot deliver a VS Code chord, VS Code's own alternative key is bound too.
  D8's `alt+q` (leave terminal focus) is the one non-VS Code key. The user may still override any key in
  `~/.config/vr-ide/keymap.toml`.
- D11 This run builds **M1–M3** (core editor, file explorer, search & replace). M4–M8 are planned in outline only.
- D12 Git: default branch `master`, remote `origin` = github.com/mvanvu/vr-ide (private). One commit per task after
  review + QC pass, pushed to `origin/master`.
- D13 Presence: user away during the build; Fable policy `auto`.

## Open questions
none
