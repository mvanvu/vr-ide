//! What a `--session` instance keeps of the AI panel (D1, D7) and how it comes back. The session
//! file (T1) stores this struct as it is; it holds only names, folders and the layout — never a
//! command line (SEC-8, SEC-18).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::spawn;
use crate::app::App;

/// Most sessions a restore reopens.
pub const MAX_SESSIONS: usize = 32;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiPanelState {
    pub visible: bool,
    /// The panel width for the session (10..=90); `None`: the setting.
    pub width_percent: Option<u16>,
    /// The active session (clamped on restore).
    pub active: usize,
    pub sessions: Vec<AiSessionState>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSessionState {
    /// The tool's name in `ai_tools`; restore requires the same name there.
    pub tool: String,
    /// The workspace folder the session ran in.
    pub root: PathBuf,
    /// The account (D11, T8); `None`: the tool's own config.
    pub account: Option<String>,
    /// The name given with AI: Rename Session.
    pub name: Option<String>,
}

/// The panel as it is now.
pub fn capture(app: &App) -> AiPanelState {
    let p = &app.ai;
    AiPanelState {
        visible: p.visible,
        width_percent: (p.width_percent != app.settings.ai_width_percent)
            .then_some(p.width_percent),
        active: p.active.min(p.sessions.len().saturating_sub(1)),
        sessions: p
            .sessions
            .iter()
            .map(|s| AiSessionState {
                tool: s.tool.clone(),
                root: s.root.clone(),
                account: s.account.clone(),
                name: s.name.clone(),
            })
            .collect(),
    }
}

/// Reopens the saved sessions, each with its tool's continue arguments (D7) and without sending
/// it any input. A session whose tool is no longer in `ai_tools` (by name) or not on `PATH`,
/// whose folder is not an existing workspace folder, or whose account cannot be used is not
/// reopened — never on another account (SEC-8, SEC-18) — and gets one error notice. The keys
/// stay where they are.
pub fn restore(app: &mut App, state: &AiPanelState) {
    if let Some(w) = state.width_percent {
        app.ai.width_percent = w.clamp(super::WIDTH_MIN, super::WIDTH_MAX);
        app.ai.width_cols = None;
    }
    let path = std::env::var_os("PATH");
    let mut opened = Vec::new();
    for (i, s) in state.sessions.iter().take(MAX_SESSIONS).enumerate() {
        match check(app, s, path.as_deref()) {
            Err(why) => app.error(format!("Not reopened: {}", why)),
            Ok(root) => {
                let tool = app
                    .settings
                    .ai_tools
                    .iter()
                    .find(|t| t.name == s.tool)
                    .cloned()
                    .expect("checked");
                let name = s.name.as_deref().and_then(|n| super::clean_name(n).ok());
                match super::spawn_session(app, &tool, None, spawn::again_mode(&tool), &root, name)
                {
                    Ok(()) => opened.push(i),
                    Err(e) => app.error(format!("Not reopened: {} ({e})", s.tool)),
                }
            }
        }
    }
    let p = &mut app.ai;
    p.picker = None;
    p.active = opened
        .iter()
        .position(|&i| i == state.active)
        .unwrap_or(0)
        .min(p.sessions.len().saturating_sub(1));
    p.visible = state.visible && !p.sessions.is_empty();
    app.dirty = true;
}

/// The folder a saved session may reopen in, or why it may not (the notice's text).
fn check(app: &App, s: &AiSessionState, path: Option<&std::ffi::OsStr>) -> Result<PathBuf, String> {
    let shown = crate::git::home_relative(&s.root);
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
    if let Some(account) = &s.account {
        let account = crate::ui::safe_text::sanitize(account);
        return Err(format!(
            "{tool_name}@{account}, account folder missing (no fallback)"
        ));
    }
    if spawn::resolve(&tool.command, path).is_none() {
        return Err(format!("{tool_name} in {shown}, {tool_name} not on PATH"));
    }
    let root = workspace_root(app, &s.root)
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
