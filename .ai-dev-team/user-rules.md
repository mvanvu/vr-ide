# User rules (kept across runs, BASE-35)

- USER-1 (user, 2026-11-24 11:27): Keybindings follow VS Code's default Linux keybindings for every feature, now and in
  later milestones. Never ask the user which keys to bind. The only non-VS Code key is `alt+q` (leave terminal focus, D8).
  Check: every command in `crates/vr-config/defaults/keymap.toml` that has a VS Code equivalent uses VS Code's default
  key; no plan, brief or message asks the user about keys.
- USER-2: vr-ide is a terminal editor — every mockup or preview shown to the user is viewable in the terminal (ANSI truecolor .ans files printed with `cat`, or the real binary), never only as HTML in a browser. HTML may exist for design QA screenshots, but the user is pointed to the terminal version. (user, 2026-11-25 03:50)
- USER-3: no mockup/design-option round for vr-ide UI work — the leader picks the layout (design QA's recommended or the designer's best) and the developer builds it directly; design QA then reviews the real build (snapshots / terminal render). (user, 2026-11-25 03:41)
- USER-4: vr-ide stays self-contained — never change other applications or system settings (terminal emulator shortcuts, gsettings, shell rc files, desktop config) to make a vr-ide feature work; vr-ide adapts instead (fallback keys, palette). (user, 2026-11-25 05:26)
- USER-5 (replaces USER-1, user 2026-11-25 05:40): vr-ide has its OWN hotkey set, one key per command, no alternative/fallback keys. Every default key must reach vr-ide from a legacy terminal (no kitty protocol) and must not be a key GNOME Terminal/VTE or the GNOME desktop takes: no ctrl+shift+<letter>, ctrl+PageUp/PageDown, ctrl+shift+PageUp/PageDown, alt+0..9, F10, F11, ctrl+plus/minus/0, alt+F1/F2/F4/F6/F7/F8, alt+space, alt+Escape, alt+`, ctrl+alt+t/d/Tab/Escape, ctrl+shift+alt+arrows; and no key a legacy terminal cannot tell apart (ctrl+i=Tab, ctrl+m=Enter, ctrl+[=Esc, ctrl+h=Backspace, ctrl+j, ctrl+2/space/@ share NUL with ctrl+`). VS Code keys are no longer the rule. Never ask the user to pick keys; propose the set, apply it, show the table.
  - USER-5 note (D229): a leader key (ctrl+k, configurable) + one letter carries every non-frequent command; Fn/Win cannot be used (never reach a terminal app).
  - USER-5 note (D230): default keys use ctrl only — no alt (and no shift+alt) anywhere.
  - USER-5 note (D231): ergonomics — defaults should sit near ctrl for the left hand; leader is ctrl+d.
