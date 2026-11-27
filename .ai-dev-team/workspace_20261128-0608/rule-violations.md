# Rule violations — run 5

- T3 BASE-49 fail (23:09): the developer ran `claude --help` once (read-only, no account touched) to confirm the `--settings` flag. Accepted by the leader: no state changed; later briefs repeat "never run the AI tools themselves".
- T2b/T5/T6 (run 5): qc added or edited temporary repo test files while the task's reviewer ran on the same tree (3 times). Fixed in later briefs.
- FR-1 R1 (03:33) COST-8 partial: one `sed -i` rename of a test field over 5 lines in ai_switch_tests.rs instead of the Edit tool; disclosed by the developer.
- FR-1 R2 (03:36) COST-8 partial: test appended with a shell `cat >>` instead of the Edit tool; disclosed.
