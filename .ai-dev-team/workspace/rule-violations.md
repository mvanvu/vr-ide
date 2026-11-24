# Rule violations ledger

| Rule | Task | What | Action |
|---|---|---|---|
| BASE-3 | T7 | tree.rs/ops.rs tests written with code, never seen red (file_index tests were) | accepted; reviewer checks those tests would fail on a broken impl; reminder in later briefs |
| BASE-3 | T3 | parser/loader tests written with code; only default-keymap tests seen red | accepted; 2nd task → warning added to brief-common |
| race (check-then-write on user files) | T7, T9 | concurrent writer overwritten/deleted in the gap; QC passed, opus review caught | warning in brief-common; qc racing-writer check per write path; T12 shared racing-writer test helper |
