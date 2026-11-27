//! The session picker (ui-spec S2, D14): the installed tools of `ai_tools`, grouped by tool,
//! then — for a tool that can pick up an earlier conversation — New / Continue last / Choose
//! earlier. It only looks for the programs on `PATH`; it never reads the tools' history.

use std::path::PathBuf;

use super::spawn::{self, StartMode};
use crate::app::App;

/// One row of the first step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A tool's name above its account rows (not selectable).
    Header { tool: String },
    /// A start: `tool` on `account` (`None`: the tool's own config, shown `default`). `single`:
    /// a tool without accounts, drawn as one row with its name. `open`: its sessions now.
    Choice {
        tool: String,
        account: Option<String>,
        open: usize,
        single: bool,
    },
}

/// The second step for the chosen tool and account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Second {
    pub tool: String,
    pub account: Option<String>,
    pub modes: Vec<StartMode>,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    /// Where the session starts: the active file's workspace folder (canonical when it exists).
    pub folder: PathBuf,
    pub rows: Vec<Row>,
    /// Tools of `ai_tools` not found on `PATH`, by name.
    pub missing: Vec<String>,
    /// Every name of `ai_tools`, for the "no tool found" line.
    pub configured: Vec<String>,
    pub selected: usize,
    pub second: Option<Second>,
    /// The tab to go back to on Esc; `None`: the picker is the empty state (Esc hides).
    pub back: Option<usize>,
}

/// Tools whose config folder can be swapped per session (D11: `CLAUDE_CONFIG_DIR`,
/// `CODEX_HOME`); they list their accounts under a header. Only `default` exists until T8.
pub fn supports_accounts(tool: &str) -> bool {
    matches!(tool, "claude" | "codex")
}

impl Picker {
    /// The rows for the tools installed now; `prefer` (the active session's tool and account)
    /// is preselected when listed, else the first start.
    pub fn build(
        app: &App,
        folder: PathBuf,
        prefer: Option<(&str, Option<&str>)>,
        back: Option<usize>,
    ) -> Picker {
        let path = std::env::var_os("PATH");
        let mut rows = Vec::new();
        let mut missing = Vec::new();
        for tool in &app.settings.ai_tools {
            if spawn::resolve(&tool.command, path.as_deref()).is_none() {
                missing.push(tool.name.clone());
                continue;
            }
            let open = app
                .ai
                .sessions
                .iter()
                .filter(|s| s.tool == tool.name && s.account.is_none())
                .count();
            let single = !supports_accounts(&tool.name);
            if !single {
                rows.push(Row::Header {
                    tool: tool.name.clone(),
                });
            }
            rows.push(Row::Choice {
                tool: tool.name.clone(),
                account: None,
                open,
                single,
            });
        }
        let first = rows
            .iter()
            .position(|r| matches!(r, Row::Choice { .. }))
            .unwrap_or(0);
        let selected = prefer
            .and_then(|(t, a)| {
                rows.iter().position(|r| {
                    matches!(r, Row::Choice { tool, account, .. }
                        if tool == t && account.as_deref() == a)
                })
            })
            .unwrap_or(first);
        Picker {
            folder,
            rows,
            missing,
            configured: app
                .settings
                .ai_tools
                .iter()
                .map(|t| t.name.clone())
                .collect(),
            selected,
            second: None,
            back,
        }
    }

    /// The selected start of the first step.
    pub fn choice(&self) -> Option<(&str, Option<&str>)> {
        match self.rows.get(self.selected)? {
            Row::Choice { tool, account, .. } => Some((tool, account.as_deref())),
            Row::Header { .. } => None,
        }
    }

    /// Up / down to the next selectable row (headers are skipped); stops at the ends.
    pub fn step(&mut self, down: bool) {
        if let Some(s) = &mut self.second {
            s.selected = if down {
                (s.selected + 1).min(s.modes.len().saturating_sub(1))
            } else {
                s.selected.saturating_sub(1)
            };
            return;
        }
        let mut i = self.selected;
        loop {
            let next = if down {
                i.checked_add(1).filter(|&n| n < self.rows.len())
            } else {
                i.checked_sub(1)
            };
            let Some(n) = next else {
                return;
            };
            i = n;
            if matches!(self.rows[i], Row::Choice { .. }) {
                self.selected = i;
                return;
            }
        }
    }
}
