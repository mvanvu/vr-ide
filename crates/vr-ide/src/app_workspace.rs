//! Workspace operations of the app (W-1): add / remove a root, open another workspace, save
//! the roots to a `.vr-workspace` file, the recent-workspaces list. W-2: the checks of Add
//! Folder and the reading of a workspace to open run off the UI thread ([`WsJob`]).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

use vr_core::{DiskVersion, Document};

use crate::app::{App, RootInfo};
use crate::commands;
use crate::commands::app::{DISCARD, SAVE_ALL};
use crate::commands::dialog::DialogAction;
use crate::commands::file::{CANCEL, SaveOutcome, save_doc};
use crate::ui::dialog::{ConfirmDialog, DialogWidget};
use crate::workspace::{Root, RootId, Workspace, WorkspaceError};

/// How long Add Folder / Open Workspace wait for their file-system checks on the UI thread.
/// A local disk answers well within it, so the result (or the error under the F1 input)
/// comes at once; a hung mount leaves the UI responsive and the result comes later.
pub const JOB_WAIT: Duration = Duration::from_millis(300);

/// What a workspace job found.
#[derive(Debug)]
pub enum JobDone {
    /// Add Folder: the folder with links resolved, or why it is refused.
    Add(Result<PathBuf, WorkspaceError>),
    /// Open Workspace / Open Recent: the workspace read, with the folders it left out.
    Open(Result<(Workspace, Vec<WorkspaceError>), WorkspaceError>),
}

/// A file-system check running on its own thread. Only the newest is kept: a second
/// request replaces it, and whatever the old thread finds is never read.
#[derive(Debug)]
pub struct WsJob {
    /// For the "still waiting" notice ("Add Folder /mnt/x").
    pub what: String,
    rx: Receiver<JobDone>,
    /// The answer, kept while a dialog or the palette is open (applying it then would
    /// replace what the user is doing).
    held: Option<JobDone>,
}

#[cfg(test)]
impl WsJob {
    /// Whether the answer has come in and waits to be applied.
    pub fn has_answer(&self) -> bool {
        self.held.is_some()
    }
}

impl App {
    /// Runs `work` on its own thread and waits [`App::ws_job_wait`] for it: `Some` with the
    /// result when it came in time, else `None` (a notice says it is still waiting, and
    /// [`App::drain_ws_job`] applies the result when it arrives).
    pub(crate) fn run_ws_job(
        &mut self,
        what: String,
        work: impl FnOnce() -> JobDone + Send + 'static,
    ) -> Option<JobDone> {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("vr-ws-check".into())
            .spawn(move || {
                let _ = tx.send(work());
            });
        if let Err(e) = spawned {
            self.error(format!("{what}: cannot start a thread: {e}"));
            return None;
        }
        // A request made while another waits replaces it; the user was promised the old
        // one, so say it will not come (D292).
        if let Some(old) = self.ws_job.take() {
            self.info(format!("{} cancelled by {what}", old.what));
        }
        match rx.recv_timeout(self.ws_job_wait) {
            Ok(done) => Some(done),
            Err(RecvTimeoutError::Disconnected) => {
                self.error(format!("{what}: the check stopped"));
                None
            }
            Err(RecvTimeoutError::Timeout) => {
                self.info(format!(
                    "{what}: waiting for the file system; it is applied when it answers"
                ));
                self.ws_job = Some(WsJob {
                    what,
                    rx,
                    held: None,
                });
                None
            }
        }
    }

    /// Applies the result of a workspace job that outlasted [`App::ws_job_wait`] (event
    /// loop; never blocks).
    /// While a dialog or the palette is open the answer is held and applied on the first
    /// pass after both are closed (D292): an Open would otherwise replace a typed Rename or
    /// a pending Save/Discard question.
    pub fn drain_ws_job(&mut self) {
        let busy = self.dialog.is_some() || self.palette.is_some();
        let Some(job) = &mut self.ws_job else {
            return;
        };
        if job.held.is_none() {
            match job.rx.try_recv() {
                Ok(d) => job.held = Some(d),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    let what = job.what.clone();
                    self.ws_job = None;
                    self.error(format!("{what}: the check stopped"));
                    return;
                }
            }
        }
        if busy {
            return;
        }
        let Some(done) = self.ws_job.take().and_then(|j| j.held) else {
            return;
        };
        let result = match done {
            JobDone::Add(r) => self.finish_add(r).map(|_| ()),
            JobDone::Open(r) => self.finish_open(r),
        };
        if let Err(e) = result {
            self.warn(format!("workspace: {e}"));
        }
        self.dirty = true;
    }

    /// Add Folder as the user asks for it: the checks that stat run off the UI thread.
    /// `None` while they still run (the result comes as a notice).
    pub fn add_root_checked(&mut self, path: &Path) -> Option<Result<RootId, WorkspaceError>> {
        let roots = self.workspace.named_paths();
        let target = path.to_path_buf();
        #[cfg(test)]
        let gate = self.ws_check_gate.take();
        let done = self.run_ws_job(format!("Add Folder {}", path.display()), move || {
            #[cfg(test)]
            if let Some(g) = gate {
                let _ = g.recv_timeout(Duration::from_secs(20));
            }
            JobDone::Add(Workspace::check_folder(&target, &roots))
        })?;
        match done {
            JobDone::Add(r) => Some(self.finish_add(r)),
            JobDone::Open(_) => None,
        }
    }

    fn finish_add(
        &mut self,
        checked: Result<PathBuf, WorkspaceError>,
    ) -> Result<RootId, WorkspaceError> {
        let id = self.workspace.add_checked(checked?)?;
        self.roots_changed();
        if let Some(r) = self.workspace.get(id) {
            let msg = format!("workspace: added {}", r.name);
            self.info(msg);
        }
        Ok(id)
    }

    /// Reads the workspace of a `.vr-workspace` file (`Some(file)`) or a folder list off the
    /// UI thread (a FIFO or a dead mount must not hang it), then opens it like
    /// [`App::open_workspace`]. `Err` when it cannot be read in time to say so now.
    pub fn open_workspace_from(
        &mut self,
        file: Option<PathBuf>,
        roots: Vec<PathBuf>,
    ) -> Result<(), WorkspaceError> {
        let what = match &file {
            Some(f) => format!("Open Workspace {}", f.display()),
            None => "Open Workspace".to_string(),
        };
        let Some(done) = self.run_ws_job(what, move || {
            JobDone::Open(match &file {
                Some(f) => Workspace::from_file(f),
                None => Workspace::from_paths(&roots),
            })
        }) else {
            return Ok(());
        };
        match done {
            JobDone::Open(r) => self.finish_open(r),
            JobDone::Add(_) => Ok(()),
        }
    }

    fn finish_open(
        &mut self,
        read: Result<(Workspace, Vec<WorkspaceError>), WorkspaceError>,
    ) -> Result<(), WorkspaceError> {
        let (ws, refused) = read?;
        for e in refused {
            self.warn(format!("workspace: {e}"));
        }
        self.open_workspace(ws);
        Ok(())
    }

    /// Reads each root's branch and whether its folder is there, for the welcome screen
    /// (with the monitor's samples and when the roots change; a frame reads only the cache).
    pub(crate) fn refresh_root_info(&mut self) {
        self.root_info = self
            .workspace
            .roots()
            .iter()
            .map(|r| RootInfo {
                path: r.path.clone(),
                branch: self.git.branch_of_root(&r.path),
                missing: r.missing(),
            })
            .collect();
    }

    /// Counts the files of each root in the go-to-file index (several roots: the index
    /// names them `root-name/relative`).
    pub fn count_root_files(&mut self) {
        self.root_files.clear();
        let Some(index) = &self.file_index else {
            return;
        };
        if !self.workspace.is_multi() {
            return;
        }
        for f in index.files() {
            if let Some(std::path::Component::Normal(name)) = f.components().next()
                && let Some(name) = name.to_str()
            {
                *self.root_files.entry(name.to_string()).or_default() += 1;
            }
        }
    }

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
        if let Some(p) = self.workspace.non_utf8_root() {
            return Err(format!(
                "{shown} not written: {}",
                WorkspaceError::NotUtf8(p.to_path_buf())
            ));
        }
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
        // Roots are canonical; the file's folder is taken the same way, so roots inside it are
        // written relative even when it is reached through a link.
        let base = path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .and_then(|d| d.canonicalize().ok())
            .zip(path.file_name())
            .map_or_else(|| path.to_path_buf(), |(d, n)| d.join(n));
        let mut doc = Document::untitled();
        doc.buffer.insert_text(&self.workspace.to_toml(&base));
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
        // TOML cannot hold such a path: the entry would fail the whole list's write (or
        // come back as another, missing folder), so this workspace is not listed.
        if let Some(p) = self.workspace.non_utf8_root() {
            let msg = format!(
                "recent workspaces: not listed; {}",
                WorkspaceError::NotUtf8(p.to_path_buf())
            );
            self.info(msg);
            return;
        }
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
        // Folders of removed roots leave `expanded`, so their watches end here too.
        commands::explorer::sync_roots(self);
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
        // Removed roots stop their git worker; new ones are discovered off the UI thread.
        crate::git::roots_changed(self);
        self.root_files.clear();
        // Branches and missing marks of new roots come with the next monitor sample.
        let paths = self.workspace.paths();
        self.root_info.retain(|i| paths.contains(&i.path));
        self.dirty = true;
    }

    fn switch_workspace(&mut self, ws: Workspace) {
        for i in (0..self.docs.len()).rev() {
            self.close_doc(i);
        }
        self.workspace = ws;
        self.roots_changed();
        // Results of the old workspace that were not searched again over the new one.
        commands::search::clear_stale(self);
        self.sync_mode();
        self.remember_workspace();
    }

    /// Roots are not replaced while a project replace writes files: the tabs it hands
    /// buffers to would close under it.
    fn replace_allows_switch(&mut self) -> bool {
        if commands::search::quit_block(self).is_none() {
            return true;
        }
        self.warn("Open Workspace: a replace is running; Esc in the find box cancels it");
        false
    }
}
