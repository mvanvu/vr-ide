# Review FR-9 (README install and usage guide)

Verdict: APPROVED (2 minor wording suggestions, not blocking)

## Findings
[minor] README.md "External changes" section - the save-time prompt is described as "the same way", but its buttons are Keep mine / Reload / Cancel (Cancel default; file.rs:143-150), and Reload first writes your unsaved text to the recovery folder (file.rs:136-148). The watcher prompt's Keep mine also makes the next save ask again (file.rs:146-148), not "replace only the version you were shown" directly.
Fix: replace the last two bullets with:
- With unsaved changes, the tab is marked and a prompt asks **Reload** (take the disk version; your text is first copied to the [recovery folder](#recovery)) or **Keep mine** (keep your buffer; saving then asks again before replacing the file).
- If you save a file that changed on disk since you opened it, the save stops and asks **Keep mine** (replace the disk version with yours), **Reload** (take the disk version, your text goes to the recovery folder first) or **Cancel** (the default); vr-ide never silently overwrites someone else's version.

[minor] README.md "Quit guard" - while a project replace is still running, quitting asks "Quit anyway" / "Cancel" (search.rs, tested at 1909). Optional addition: "If a project replace is still running, ctrl+q asks Quit anyway or Cancel (Cancel is the default)."

## Files read
README.md (full + diff), 04_dev_FR-9.md, 02_tasks.md row FR-9, crates/vr-ide/src/{cli,main,clipboard,app,recovery}.rs, commands/{file,search,find,explorer,nav,app}.rs (relevant parts), crates/vr-config/defaults/{keymap,settings}.toml, docs/keymap.md (head), crates/vr-config/src/paths.rs (grep).

## Rule check FR-9
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only README.md changed (git status) |
| BASE-6 | pass | English; no machine-specific absolute paths (grep) |
| BASE-32 | pass | keys match defaults/keymap.toml (D10) |
| AC-1 | pass | CLI args, root resolution, messages and exit 2 match cli.rs/main.rs |
| AC-2 | pass | install/build/uninstall commands; package name vr-ide; no-flags behaviour matches |
Old README facts (recovery path, env override, startup notice) all retained.
