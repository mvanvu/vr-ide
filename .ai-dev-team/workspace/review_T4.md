# Review T4 (opus) — CHANGES_REQUESTED
1. [blocker] vr-window app.rs:105 + vr-ide main.rs:136-141: run_app Err after the session started (display lost: Wayland flush/dispatch error, X11 reset) is treated as StartError → "running in the terminal instead" → enter_terminal fails without a tty (menu launch) → main returns Err without event_loop::exit/finish → unsaved buffers lost, no recovery, session not saved, terminals/AI not shut down.
2. [major] app.rs:284-315,178-193: IME cursor area not re-sent on Focused(true), Ime::Enabled, or after the cancel reset (Wayland text-input enable resets state) → candidate popup misplaced (AC-W5).
3. [minor] window_frontend.rs:281-288: signal-hook unregister leaves SIGTERM ignored, not default (comment wrong).
4. [minor, user-visible] main.rs:81-84: TUI fallback after a failed window start keeps true colour in a 256-colour terminal.
Rule check: AC-W5 fail, AC-W8 fail, BASE-2 fail, others pass; deps pass, no GTK.
Re-check round 1: APPROVED. New minor: SIGTERM default switched on before finish() in the display-lost path. X11 IO error: recommended emergency recovery via XSetIOErrorHandler hook (~45 lines) → follow-up T4b.
Re-check round 2: APPROVED (X11 IO hook safe; exit_with contract).
