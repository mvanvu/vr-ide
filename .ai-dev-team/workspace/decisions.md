# Decisions — run 6

- D1 (06:09) Fable policy while away: carried over from run 5 (auto). confirm-on-return.
- D2 (06:09) Commits: one local commit per task; no push until the user confirms. confirm-on-return.
- D3 (06:09) New crate crates/vr-window for renderer, input and runner (keeps App out, lets tasks run in parallel per crate).
- D4 (06:09) Phase 5 dimensions: correctness, ui (no threat model: spec security_sensitive false; T5 reviewed as high-risk because it overwrites a user file).
