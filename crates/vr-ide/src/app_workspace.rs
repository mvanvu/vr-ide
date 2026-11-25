//! Workspace operations of the app (W-1): add / remove a root, open another workspace, save
//! the roots to a `.vr-workspace` file, the recent-workspaces list. The palette commands and
//! the explorer's root nodes that call them are W-2's.

use std::path::Path;

use vr_core::{DiskVersion, Document};

use crate::app::App;
use crate::commands;
use crate::commands::app::{DISCARD, SAVE_ALL};
use crate::commands::dialog::DialogAction;
use crate::commands::file::{CANCEL, SaveOutcome, save_doc};
use crate::ui::dialog::{ConfirmDialog, DialogWidget};
use crate::workspace::{Root, RootId, Workspace, WorkspaceError};

impl App {
    /// Adds the folder `path` as the last root: go to file, project search and the folder
    /// watches include it at once. Nested or repeated folders are refused.
    pub fn add_root(&mut self, path: &Path) -> Result<RootId, WorkspaceError> {
        let id = self.workspace.add(path)?;
        self.roots_changed();
        Ok(id)
    }

    /// Removes a root from the workspace. Nothing on disk is touched and nothing is closed:
    /// tabs on its files keep working (shown with their full path). The last root stays.
    pub fn remove_root(&mut self, id: RootId) -> Result<Root, WorkspaceError> {
        let root = self.workspace.remove(id)?;
        self.roots_changed();
        Ok(root)
    }

    /// Replaces the roots with `ws`, after the unsaved-files question quit asks (Save all /
    /// Discard / Cancel); every tab is closed, terminals keep running. Refused while a
    /// project replace writes files.
    pub fn open_workspace(&mut self, ws: Workspace) {
        if !self.replace_allows_switch() {
            return;
        }
        let unsaved: Vec<String> = self
            .docs
            .iter()
            .filter(|d| d.is_modified())
            .map(|d| {
                d.path
                    .as_deref()
                    .map_or_else(|| "untitled".to_string(), |p| self.display_path(p))
            })
            .collect();
        if unsaved.is_empty() {
            self.switch_workspace(ws);
            return;
        }
        let message = format!(
            "{} unsaved file(s): {}. Discard keeps a copy in the recovery folder.",
            unsaved.len(),
            unsaved.join(", ")
        );
        self.open_dialog(
            DialogWidget::Confirm(ConfirmDialog::new(
                "Open Workspace",
                message,
                &[SAVE_ALL, DISCARD, CANCEL],
            )),
            DialogAction::OpenWorkspace(Box::new(ws)),
        );
    }

    /// The answer to [`App::open_workspace`]'s question. Save all: switches once every
    /// modified file is saved (stops at a failure or a prompt). Discard: switches once the
    /// buffers are in the recovery folder; when that write fails nothing is discarded.
    pub(crate) fn answer_open_workspace(&mut self, ws: Workspace, button: usize) {
        if !self.replace_allows_switch() {
            return;
        }
        match button {
            0 => {
                for i in 0..self.docs.len() {
                    if self.docs[i].is_modified() && save_doc(self, i) != SaveOutcome::Saved {
                        return;
                    }
                }
                self.switch_workspace(ws);
            }
            1 => {
                let all: Vec<usize> = (0..self.docs.len()).collect();
                match self.keep_discarded(&all) {
                    Ok(files) => {
                        self.switch_workspace(ws);
                        if !files.is_empty() {
                            let list: Vec<String> =
                                files.iter().map(|f| f.display().to_string()).collect();
                            self.info(format!(
                                "unsaved changes were kept in the recovery folder: {}",
                                list.join(", ")
                            ));
                        }
                    }
                    Err(e) => self.error(e),
                }
            }
            _ => {}
        }
    }

    /// Writes the roots to the workspace file `path`. `seen`: the version of the file the
    /// user agreed to replace (take it with `DiskVersion::of_path` when asking); `None`
    /// creates the file and refuses an existing one. A file changed since `seen` is never
    /// overwritten (vr-core's freshness-checked Save As does the write).
    pub fn save_workspace(
        &mut self,
        path: &Path,
        seen: Option<&DiskVersion>,
    ) -> Result<(), String> {
        let shown = path.display();
        let fresh;
        let seen = match seen {
            Some(v) => v,
            None => {
                if path.symlink_metadata().is_ok() {
                    return Err(format!(
                        "{shown} exists; /ws save does not replace another file"
                    ));
                }
                // An absent file is a version too: one created meanwhile is refused.
                fresh = DiskVersion::of_path(path).map_err(|e| format!("{shown}: {e}"))?;
                &fresh
            }
        };
        let mut doc = Document::untitled();
        doc.buffer.insert_text(&self.workspace.to_toml(path));
        doc.save_as_seen(path, seen)
            .map_err(|e| format!("{shown}: {e}"))?;
        let file = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        // What was just written is the version the next save onto this file may replace.
        // (A write landing between the save and this read would be taken as ours.)
        let version = DiskVersion::of_path(path).ok();
        self.workspace.set_file_with_version(file, version);
        self.remember_workspace();
        self.dirty = true;
        Ok(())
    }

    /// Puts the current workspace in front of the recent list and writes the list. The file
    /// is read again first, so workspaces another vr-ide recorded meanwhile are kept.
    pub fn remember_workspace(&mut self) {
        let entry = self.workspace.recent_entry();
        let Some(file) = self.recent_workspaces_file.clone() else {
            self.recent_workspaces.push(entry);
            return;
        };
        let (mut list, _) = vr_config::RecentWorkspaces::load(&file);
        list.push(entry);
        if let Err(e) = list.save(&file) {
            self.warn(format!(
                "recent workspaces: cannot write {}: {e}",
                file.display()
            ));
        }
        self.recent_workspaces = list;
    }

    /// Brings the explorer, the folder watches, go to file and a shown search in step with
    /// the roots.
    pub(crate) fn roots_changed(&mut self) {
        if self.explorer.root() != self.root() {
            let fresh = self.explorer.rerooted(&self.workspace.primary().path);
            let old = std::mem::replace(&mut self.explorer, fresh);
            if let Some(w) = &mut self.watcher {
                for p in &old.watched {
                    w.unwatch_dir(p);
                }
            }
            if old.is_loaded() {
                commands::explorer::ensure_loaded(self);
            }
        }
        self.sync_explorer_watches();
        if self.file_index.is_some() || self.index_rx.is_some() {
            // Dropping the receiver stops a build of the old roots.
            self.file_index = None;
            self.index_rx = None;
            self.start_index();
            if self.palette.is_some() {
                commands::palette::refresh_keeping_selection(self);
            }
        }
        commands::search::roots_changed(self);
        self.dirty = true;
    }

    fn switch_workspace(&mut self, ws: Workspace) {
        for i in (0..self.docs.len()).rev() {
            self.close_doc(i);
        }
        self.workspace = ws;
        self.roots_changed();
        self.sync_mode();
        self.remember_workspace();
    }

    /// Roots are not replaced while a project replace writes files: the tabs it hands
    /// buffers to would close under it.
    fn replace_allows_switch(&mut self) -> bool {
        if commands::search::quit_block(self).is_none() {
            return true;
        }
        self.warn("Open Workspace: a replace is running; Esc in the search panel cancels it");
        false
    }
}
