# Review T5 (opus) — CHANGES_REQUESTED
1. [major] desktop.rs:75-76 launches_in_window() always true before the window exists → Terminal=false entry for a TUI-only binary. (Leader's brief caused it.)
2. [minor] desktop.rs:224 Terminal=true entry (non-GNOME) never migrated → extra empty terminal each launch.
3. [low] check_target→rename race (CWE-367), pre-existing in install; no change.
Rule check: AC-W7 fail (finding 1), BASE-13/2/3/9/16/20/23/50 pass.
Re-check round 1: APPROVED (both fixed; T4 must switch launches_in_window).
