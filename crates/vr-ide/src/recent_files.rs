//! Writes the recent-files list (`/recent`, `vr_config::RecentFiles`) on a thread of its own,
//! so opening a file never waits for the state folder (a slow or network home folder).
//!
//! Each recorded file is merged into what the file holds at that moment (another vr-ide may
//! have written it meanwhile) under the list's lock, and the list is written atomically. Paths
//! are recorded in order through one channel, so the last opened file is always the newest
//! entry.

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

enum Msg {
    Record(PathBuf),
    Renamed(PathBuf, PathBuf),
    Flush(Sender<()>),
}

pub struct RecentWriter {
    tx: Sender<Msg>,
    error: Arc<Mutex<Option<String>>>,
}

impl RecentWriter {
    /// Starts the writer for the list kept in `file`. The thread ends when the writer is
    /// dropped; at exit [`Self::close`] waits (bounded) for what was recorded.
    pub fn start(file: PathBuf) -> RecentWriter {
        let (tx, rx) = mpsc::channel::<Msg>();
        let error = Arc::new(Mutex::new(None));
        let seen = error.clone();
        let spawned = std::thread::Builder::new()
            .name("vr-recent-files".into())
            .spawn(move || {
                for msg in rx {
                    let done = match msg {
                        Msg::Record(path) => vr_config::RecentFiles::record(&file, path),
                        Msg::Renamed(from, to) => {
                            vr_config::RecentFiles::update(&file, |l| l.renamed(&from, to))
                        }
                        Msg::Flush(done) => {
                            let _ = done.send(());
                            Ok(())
                        }
                    };
                    if let Err(text) = done {
                        *seen.lock().unwrap_or_else(|p| p.into_inner()) = Some(text);
                    }
                }
            });
        if let Err(e) = spawned {
            *error.lock().unwrap_or_else(|p| p.into_inner()) =
                Some(format!("cannot start the writer: {e}"));
        }
        RecentWriter { tx, error }
    }

    /// Queues `path` as the newest entry; never blocks.
    pub fn record(&self, path: PathBuf) {
        let _ = self.tx.send(Msg::Record(path));
    }

    /// Queues a rename: `from` leaves the list, `to` becomes the newest entry.
    pub fn renamed(&self, from: PathBuf, to: PathBuf) {
        let _ = self.tx.send(Msg::Renamed(from, to));
    }

    /// Waits until everything recorded before is written.
    pub fn flush(&self) {
        let (done, wait) = mpsc::channel();
        if self.tx.send(Msg::Flush(done)).is_ok() {
            let _ = wait.recv();
        }
    }

    /// At exit: waits up to `wait` for what was recorded before, so a file opened just before
    /// quitting is in the list; a stuck state folder cannot hold the exit longer than that.
    pub fn close(&self, wait: Duration) {
        let (done, rx) = mpsc::channel();
        if self.tx.send(Msg::Flush(done)).is_ok() {
            let _ = rx.recv_timeout(wait);
        }
    }

    /// The last write error, once.
    pub fn take_error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn close_writes_what_was_recorded_just_before() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join(vr_config::RECENT_FILES_FILE);
        let w = RecentWriter::start(file.clone());
        w.record(PathBuf::from("/x.rs"));
        // `close` returns as soon as the writer has answered; the bound only caps a stuck
        // writer, so a generous one keeps the test from depending on machine load.
        w.close(Duration::from_secs(30));
        let (list, _) = vr_config::RecentFiles::load(&file);
        assert_eq!(list.entries, vec![PathBuf::from("/x.rs")]);
    }
}
