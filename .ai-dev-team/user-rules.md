# User rules (kept across runs, BASE-35)

- USER-1 (user, 2026-11-24 11:27): Keybindings follow VS Code's default Linux keybindings for every feature, now and in
  later milestones. Never ask the user which keys to bind. The only non-VS Code key is `alt+q` (leave terminal focus, D8).
  Check: every command in `crates/vr-config/defaults/keymap.toml` that has a VS Code equivalent uses VS Code's default
  key; no plan, brief or message asks the user about keys.
- USER-2: vr-ide is a terminal editor — every mockup or preview shown to the user is viewable in the terminal (ANSI truecolor .ans files printed with `cat`, or the real binary), never only as HTML in a browser. HTML may exist for design QA screenshots, but the user is pointed to the terminal version. (user, 2026-11-25 03:50)
