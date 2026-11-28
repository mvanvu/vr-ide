# Decisions — run 6

- D1 (06:09) Fable policy while away: carried over from run 5 (auto). confirm-on-return.
- D2 (06:09) Commits: one local commit per task; no push until the user confirms. confirm-on-return.
- D3 (06:09) New crate crates/vr-window for renderer, input and runner (keeps App out, lets tasks run in parallel per crate).
- D4 (06:09) Phase 5 dimensions: correctness, ui (no threat model: spec security_sensitive false; T5 reviewed as high-risk because it overwrites a user file).
- D5 (06:27, T5) Only the gnome-terminal form of a vr-ide-written menu entry is migrated; a Terminal=true entry (non-GNOME desktops) and an entry whose TryExec names another binary are left alone (AC-W7 names the gnome-terminal form; avoids a dev build hijacking the menu entry).
- D5 revised (06:29): the Terminal=true form of a vr-ide-written entry with the same exe is migrated too (reviewer minor; avoids an empty terminal on non-GNOME desktops). TryExec of another binary still skipped. confirm-on-return.
- D6 (07:34, T4) TUI fallback after a failed window start must use the terminal's real colour depth (reviewer minor 4, user-visible): fixed in the T4 fix round. confirm-on-return.
- D7 (08:29, FR-1) In the window SIGHUP is ignored (the window does not need the launching terminal; closing it must not close the editor); SIGINT takes the quit path like SIGTERM. confirm-on-return.
