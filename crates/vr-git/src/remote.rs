//! Push and pull in the background (threat model SEC-13, SEC-14): progress lines, cancel,
//! an inactivity limit, no prompts of any kind, no force, credentials hidden in every text.

use std::ffi::{OsStr, OsString};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::cmd::{Cancel, Ctl, Limits, LineSink, Output, Profile};
use crate::parse::{lossy, os};
use crate::repo::Head;
use crate::sanitize::{redact_userinfo, sanitize_line};
use crate::state::{Operation, RepoState};
use crate::write::Writer;
use crate::{GitError, Result};

/// Lines of output kept for the result.
const KEEP_LINES: usize = 200;

/// Where a push goes; shown to the user before it runs (SEC-16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushTarget {
    pub remote: String,
    /// The local branch (HEAD).
    pub branch: String,
    pub remote_branch: String,
    /// The branch has no upstream yet: `--set-upstream` makes this one its upstream.
    pub set_upstream: bool,
}

impl std::fmt::Display for PushTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.remote, self.remote_branch)
    }
}

/// What a background job reports.
#[derive(Debug, Clone)]
pub enum JobEvent {
    /// One progress or message line (credentials redacted, control characters visible).
    Progress(String),
    Done(Result<JobOutcome>),
}

/// A finished push or pull. A pull that stopped on conflicts is an outcome too: `state` says
/// so (SEC-14) and `output` has git's explanation; resolution stays in the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobOutcome {
    pub output: Vec<String>,
    pub state: RepoState,
}

/// A running push or pull.
pub struct Job {
    cancel: Cancel,
    rx: Receiver<JobEvent>,
    handle: Option<JoinHandle<()>>,
}

impl Job {
    /// Kill the running git (and its ssh, helpers, hooks).
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn canceller(&self) -> Cancel {
        self.cancel.clone()
    }

    /// The next event, without blocking.
    pub fn try_recv(&self) -> Option<JobEvent> {
        self.rx.try_recv().ok()
    }

    /// Block until the job ends; progress lines are dropped.
    pub fn wait(mut self) -> Result<JobOutcome> {
        let mut result = Err(GitError::Io("job ended without a result".into()));
        for e in self.rx.iter() {
            if let JobEvent::Done(r) = e {
                result = r;
            }
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        result
    }
}

fn clean_text(bytes: &[u8]) -> String {
    sanitize_line(&redact_userinfo(&lossy(bytes)))
}

impl Writer {
    /// The current branch, the remote it pushes to and the branch there: `pushRemote`,
    /// `remote.pushDefault`, the upstream's remote, `origin`, or the only remote. Refused on a
    /// detached or unborn HEAD.
    pub fn push_target(&self) -> Result<PushTarget> {
        let branch = match self.repo.head()? {
            Head::Branch { name, oid: Some(_) } => name,
            Head::Branch { oid: None, .. } => {
                return Err(GitError::Refused("the branch has no commits yet".into()));
            }
            Head::Detached { .. } => {
                return Err(GitError::Refused(
                    "HEAD is detached; push from the terminal".into(),
                ));
            }
        };
        let remotes: Vec<String> = lossy(&self.repo.run(&[os("remote")], false)?)
            .lines()
            .map(str::to_string)
            .collect();
        let upstream_remote = self.config(&format!("branch.{branch}.remote"))?;
        let merge = self.config(&format!("branch.{branch}.merge"))?;
        let remote = match (
            self.config(&format!("branch.{branch}.pushRemote"))?,
            self.config("remote.pushDefault")?,
            upstream_remote.clone(),
        ) {
            (Some(r), _, _) | (None, Some(r), _) | (None, None, Some(r)) => r,
            _ if remotes.iter().any(|r| r == "origin") => "origin".to_string(),
            _ if remotes.len() == 1 => remotes[0].clone(),
            _ => return Err(GitError::Refused("no remote to push to".into())),
        };
        if remote.starts_with('-') || !remotes.contains(&remote) {
            return Err(GitError::Refused(format!(
                "{remote} is not a configured remote"
            )));
        }
        let tracked = upstream_remote.as_deref() == Some(remote.as_str());
        let remote_branch = match merge.as_deref().and_then(|m| m.strip_prefix("refs/heads/")) {
            Some(b) if tracked => b.to_string(),
            _ => branch.clone(),
        };
        let valid = self.repo.run_code(&[
            os("check-ref-format"),
            os(&format!("refs/heads/{remote_branch}")),
        ])?;
        if valid.code != 0 {
            return Err(GitError::Refused(format!(
                "{remote_branch} is not a valid branch name"
            )));
        }
        Ok(PushTarget {
            remote,
            branch,
            remote_branch,
            // Only a branch without any upstream gets one: a fork setup (pushRemote or
            // remote.pushDefault) keeps pulling from its own upstream (D305(2)).
            set_upstream: upstream_remote.is_none() && merge.is_none(),
        })
    }

    fn config(&self, key: &str) -> Result<Option<String>> {
        let out = self.repo.run_code(&[os("config"), os("--get"), os(key)])?;
        Ok((out.code == 0).then(|| lossy(out.stdout.trim_ascii_end())))
    }

    /// Push the current branch to `target` (from [`Writer::push_target`], shown to the user).
    /// Never forced: the refspec is fixed and has no `+`.
    pub fn push(&self, target: PushTarget, notify: impl Fn() + Send + 'static) -> Job {
        self.job(notify, move |w, cancel, sink| {
            let _g = w.lock.acquire(Some(cancel))?;
            let state = w.state()?;
            if let Some(op @ (Operation::Rebase | Operation::Am)) = state.operation {
                return Err(GitError::Refused(format!(
                    "a {} is in progress; finish it in the terminal",
                    op.name()
                )));
            }
            match w.repo.head()? {
                Head::Branch { name, .. } if name == target.branch => {}
                _ => return Err(GitError::Stale("the current branch".into())),
            }
            if target.remote.starts_with('-') {
                return Err(GitError::Refused("bad remote name".into()));
            }
            let refspec = format!(
                "refs/heads/{}:refs/heads/{}",
                target.branch, target.remote_branch
            );
            let mut args: Vec<&OsStr> = vec![os("push"), os("--progress")];
            if target.set_upstream {
                args.push(os("--set-upstream"));
            }
            args.extend([os("--end-of-options"), os(&target.remote), os(&refspec)]);
            w.network(&args, cancel, sink)
        })
    }

    /// `git pull --no-edit` (merge or rebase as the user's config says). Refused while an
    /// operation or a conflict is in progress; a pull that stops on conflicts reports them in
    /// the outcome's state.
    pub fn pull(&self, notify: impl Fn() + Send + 'static) -> Job {
        self.job(notify, move |w, cancel, sink| {
            let _g = w.lock.acquire(Some(cancel))?;
            let state = w.state()?;
            if !state.is_clean() {
                return Err(GitError::Refused(format!(
                    "{state}; finish it in the terminal before pulling"
                )));
            }
            w.network(
                &[os("pull"), os("--no-edit"), os("--progress")],
                cancel,
                sink,
            )
        })
    }

    fn network(&self, args: &[&OsStr], cancel: &Cancel, sink: &Sink) -> Result<JobOutcome> {
        let mut env: Vec<(OsString, OsString)> = Vec::new();
        let has_ssh = self.repo.git.env_var("GIT_SSH_COMMAND").is_some()
            || self.repo.git.env_var("GIT_SSH").is_some()
            || self.config("core.sshCommand")?.is_some();
        if !has_ssh {
            // No passphrase or host-key question can reach a terminal (SEC-13).
            env.push(("GIT_SSH_COMMAND".into(), "ssh -o BatchMode=yes".into()));
        }
        let o = self.repo.git.opts();
        // No wall-clock limit (a large push may take long); silence is the limit.
        let lim = Limits::new(Duration::from_secs(7 * 86_400), o.max_output).with(Profile::Network);
        let ctl = Ctl {
            cancel: Some(cancel.clone()),
            idle: Some(o.network_idle_timeout),
            on_line: Some(sink.line.clone()),
            env,
        };
        let out: Output = self.run_write(args, None, lim, &ctl)?;
        for l in out.stdout.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
            (sink.line)(l);
        }
        let state = self.state()?;
        let output = sink.kept.lock().unwrap().clone();
        if out.code == 0 || !state.is_clean() {
            return Ok(JobOutcome { output, state });
        }
        Err(GitError::Failed {
            cmd: lossy(args[0].as_encoded_bytes()),
            code: out.code,
            stderr: out
                .stderr
                .lines()
                .map(|l| clean_text(l.as_bytes()))
                .collect::<Vec<_>>()
                .join("\n"),
        })
    }

    fn job(
        &self,
        notify: impl Fn() + Send + 'static,
        work: impl FnOnce(&Writer, &Cancel, &Sink) -> Result<JobOutcome> + Send + 'static,
    ) -> Job {
        let (tx, rx) = mpsc::channel();
        let cancel = Cancel::new();
        let notify = Mutex::new(notify);
        let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || (notify.lock().unwrap())());
        let kept = Arc::new(Mutex::new(Vec::new()));
        let line: LineSink = {
            let (tx, notify, kept) = (Mutex::new(tx.clone()), notify.clone(), kept.clone());
            Arc::new(move |bytes: &[u8]| {
                let text = clean_text(bytes);
                {
                    let mut k = kept.lock().unwrap();
                    if k.len() >= KEEP_LINES {
                        k.remove(0);
                    }
                    k.push(text.clone());
                }
                let _ = tx.lock().unwrap().send(JobEvent::Progress(text));
                notify();
            })
        };
        let sink = Sink { line, kept };
        let (w, c) = (self.clone(), cancel.clone());
        let handle = std::thread::spawn(move || {
            let r = work(&w, &c, &sink);
            let _ = tx.send(JobEvent::Done(r));
            notify();
        });
        Job {
            cancel,
            rx,
            handle: Some(handle),
        }
    }
}

struct Sink {
    line: LineSink,
    kept: Arc<Mutex<Vec<String>>>,
}
