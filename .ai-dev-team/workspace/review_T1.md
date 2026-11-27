# Review T1 (sonnet) — APPROVED
- [minor] event_loop.rs ~324: `attention_marks` compares a count; a mark cleared and another added in the same pass gives no request_attention. Carried to T4 (window attention) as a note; else Phase 6 minor.
Rule check: AC-W1 pass, BASE-2/3/1/16/32/35/50 pass, BASE-9 and COST n-o.
