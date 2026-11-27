# T4a notes
Files: crates/vr-config/src/settings.rs, crates/vr-config/defaults/settings.toml, crates/vr-config/tests/config.rs, crates/vr-term/src/terminal.rs.

## API for T4
- `Settings.font_family: String` (default "", max 100 chars, no control chars), `font_size: f32` (11.0, 6..=72), `line_height: f32` (1.2, 1.0..=2.0). Invalid -> `ConfigWarning::InvalidValue { key }`, defaults kept (same path as other keys, so reload covers them).
- `Settings` no longer derives `Eq` (floats); `PartialEq` stays. Workspace compiles.
- `Terminal::resize_with_pixels(cols, rows, cell_w, cell_h)`: PTY ws_xpixel = cols*cell_w, ws_ypixel = rows*cell_h (u16 saturating). `resize(cols, rows)` = 0 pixels.
- Not touched: `/set` listing in vr-ide (no per-key list found for settings; T4 owns vr-ide).

## Tests
- vr-config: `font_settings_defaults_valid_and_invalid` (28 passed in config.rs). vr-term: `pty_size_multiplies_and_saturates`, `resize_with_pixels_reaches_the_pty_and_resize_clears_them` (reads TIOCGWINSZ via master.get_size()); all vr-term suites pass.
- clippy -p vr-config -p vr-term --all-targets -D warnings: clean.
- Guard `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`: passed (only a manifest warning: unused dependency winit in crates/vr-window, T3's).

## Rule compliance
BASE: pass (no stage/commit/checkout; guard run exactly). COST: pass (nothing left running). AC-W6 PTY pixels: pass. AC-W8 font fallback warning: pass for invalid values (settings side; uninstalled-family fallback is T4/T3).

## Quick pass
Added nan, inf, negative cases to font_settings_defaults_valid_and_invalid; vr-config tests, clippy and guard pass.
