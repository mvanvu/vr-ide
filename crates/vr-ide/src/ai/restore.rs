//! What a `--session` window keeps of the AI panel (D1, D7) and how it comes back: the
//! session file's `[ai]` section ([`AiSection`], T1) holds only tool names, account names,
//! folders, labels and the layout — never a command line (SEC-8, SEC-18).
//!
//! [`from_session`] runs once at start, after the session's workspace roots and tabs are back;
//! from then on [`report`] hands the panel's state to the session store before each save.

use std::path::{Path, PathBuf};

use super::spawn;
use crate::app::App;
use crate::session::{self, AiSection, AiSession};

/// The panel as the session file keeps it.
pub fn capture(app: &App) -> AiSection {
    let p = &app.ai;
    AiSection {
        shown: p.visible,
        width: (p.width_percent != app.settings.ai_width_percent).then_some(p.width_percent),
        sessions: p
            .sessions
            .iter()
            .map(|s| AiSession {
                tool: s.tool.clone(),
                account: s.account.clone().unwrap_or_default(),
                folder: s.root.clone(),
                label: s.name.clone().unwrap_or_default(),
            })
            .collect(),
    }
}

/// Reopens the AI section the session file was read with (once), then keeps the file in step
/// with the panel. A window without `--session` has no section and reports nothing.
pub fn from_session(app: &mut App) {
    if app.session.is_none() {
        return;
    }
    if let Some(state) = session::take_restored_ai(app) {
        restore(app, &state);
    }
    // What could not be reopened is not written back.
    let now = capture(app);
    session::set_ai(app, now.clone());
    app.ai.reported = Some(now);
}

/// Hands the panel's state to the session store when it changed since the last call (the
/// store's save checks and the quit save call this first). Nothing before [`from_session`].
pub fn report(app: &mut App) {
    let Some(last) = &app.ai.reported else {
        return;
    };
    let now = capture(app);
    if *last != now {
        session::set_ai(app, now.clone());
        app.ai.reported = Some(now);
    }
}

/// Reopens the saved sessions, each with its tool's continue arguments (D7) and without sending
/// it any input. A session whose tool is no longer in `ai_tools` (by name) or not on `PATH`,
/// whose folder is not an existing workspace folder, or which names an account is not
/// reopened — never on another account (SEC-8, SEC-18) — and gets one error notice. The keys
/// stay where they are.
pub fn restore(app: &mut App, state: &AiSection) {
    if let Some(w) = state.width {
        app.ai.width_percent = w.clamp(super::WIDTH_MIN, super::WIDTH_MAX);
        app.ai.width_cols = None;
    }
    let path = std::env::var_os("PATH");
    for s in state.sessions.iter().take(session::MAX_AI_SESSIONS) {
        match check(app, s, path.as_deref()) {
            Err(why) => app.error(format!("Not reopened: {why}")),
            Ok(root) => {
                let tool = app
                    .settings
                    .ai_tools
                    .iter()
                    .find(|t| t.name == s.tool)
                    .cloned()
                    .expect("checked");
                let name = Some(s.label.as_str())
                    .filter(|l| !l.is_empty())
                    .and_then(|n| super::clean_name(n).ok());
                if let Err(e) =
                    super::spawn_session(app, &tool, None, spawn::again_mode(&tool), &root, name)
                {
                    app.error(format!("Not reopened: {} ({e})", s.tool));
                }
            }
        }
    }
    let p = &mut app.ai;
    p.picker = None;
    p.active = 0;
    p.visible = state.shown && !p.sessions.is_empty();
    app.dirty = true;
}

/// The folder a saved session may reopen in, or why it may not (the notice's text).
fn check(app: &App, s: &AiSession, path: Option<&std::ffi::OsStr>) -> Result<PathBuf, String> {
    let shown = crate::git::home_relative(&s.folder);
    let tool_name = if vr_config::valid_tool_name(&s.tool) {
        s.tool.clone()
    } else {
        crate::ui::safe_text::sanitize(&s.tool)
    };
    let Some(tool) = app.settings.ai_tools.iter().find(|t| t.name == s.tool) else {
        return Err(format!(
            "{tool_name} in {shown}, {tool_name} is not in ai_tools"
        ));
    };
    // Accounts come with T8; until then a session on one is never moved to the default.
    if !s.account.is_empty() {
        let account = crate::ui::safe_text::sanitize(&s.account);
        return Err(format!(
            "{tool_name}@{account}, account folder missing (no fallback)"
        ));
    }
    if spawn::resolve(&tool.command, path).is_none() {
        return Err(format!("{tool_name} in {shown}, {tool_name} not on PATH"));
    }
    let root = workspace_root(app, &s.folder)
        .ok_or_else(|| format!("{tool_name} in {shown} (folder missing)"))?;
    Ok(root)
}

/// `root` when it is one of the workspace's folders and exists, canonical.
fn workspace_root(app: &App, root: &Path) -> Option<PathBuf> {
    let canonical = root.canonicalize().ok().filter(|p| p.is_dir())?;
    app.workspace
        .roots()
        .iter()
        .any(|r| r.path == root || r.path.canonicalize().is_ok_and(|p| p == canonical))
        .then_some(canonical)
}
