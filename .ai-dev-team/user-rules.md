# User rules (kept across runs, BASE-35)

- USER-1 (user, 2026-11-24 11:27): Keybindings follow VS Code's default Linux keybindings for every feature, now and in
  later milestones. Never ask the user which keys to bind. The only non-VS Code key is `alt+q` (leave terminal focus, D8).
  Check: every command in `crates/vr-config/defaults/keymap.toml` that has a VS Code equivalent uses VS Code's default
  key; no plan, brief or message asks the user about keys.
