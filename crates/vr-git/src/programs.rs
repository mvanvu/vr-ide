//! What in a repository's own config makes git run a program when the user writes
//! (threat model SEC-19, D303): reads never run these (SEC-3), but a user's stage, commit,
//! push or pull runs them exactly as git in the terminal would, so the UI names them first.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::Result;
use crate::parse::{fields, lossy, os};
use crate::repo::Repo;

impl Repo {
    /// Program-running keys of the repository's local config (`git config --local
    /// --includes`, so an included file counts) and its executable hooks, e.g.
    /// `filter.lfs.clean`, `commit.gpgsign`, `hook pre-commit`. Key names are as git prints
    /// them (section and key lower case); they come from the repository, so a UI sanitizes
    /// them before drawing. Empty for a repo whose config runs nothing.
    pub fn write_programs(&self) -> Result<Vec<String>> {
        let out = self.run_code(&[
            os("config"),
            os("--local"),
            os("--includes"),
            os("-z"),
            os("--list"),
        ])?;
        let mut found: Vec<String> = Vec::new();
        let mut hooks_path: Option<String> = None;
        if out.code == 0 {
            for rec in fields(&out.stdout) {
                let rec = lossy(rec);
                let (key, value) = rec.split_once('\n').unwrap_or((rec.as_str(), ""));
                if key == "core.hookspath" {
                    hooks_path = Some(value.to_string());
                }
                if runs_program(key, value) && !found.iter().any(|k| k == key) {
                    found.push(key.to_string());
                }
            }
        }
        let dir = match hooks_path.as_deref() {
            Some(p) if !p.is_empty() => {
                let p = PathBuf::from(p);
                if p.is_absolute() {
                    p
                } else {
                    self.workdir().join(p)
                }
            }
            _ => self.common_dir().join("hooks"),
        };
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut hooks: Vec<String> = rd
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let meta = std::fs::metadata(e.path()).ok()?;
                    let exec = meta.is_file() && meta.permissions().mode() & 0o111 != 0;
                    (exec && !name.ends_with(".sample")).then(|| format!("hook {name}"))
                })
                .collect();
            hooks.sort();
            found.extend(hooks);
        }
        Ok(found)
    }
}

/// A config key (as `git config --list` prints it) whose value names a program git runs, or
/// turns one on. `false`-valued switches do not count.
fn runs_program(key: &str, value: &str) -> bool {
    let off = matches!(
        value.to_ascii_lowercase().as_str(),
        "false" | "no" | "off" | "0" | ""
    );
    let (first, rest) = key.split_once('.').unwrap_or((key, ""));
    let last = key.rsplit('.').next().unwrap_or("");
    match first {
        "core" => match rest {
            "fsmonitor" | "gpgsign" => !off,
            "hookspath" | "sshcommand" | "pager" | "askpass" | "editor" | "gitproxy" => true,
            _ => false,
        },
        "filter" => matches!(last, "clean" | "smudge" | "process"),
        "diff" => rest == "external" || matches!(last, "textconv" | "command"),
        "merge" => last == "driver",
        "commit" | "tag" | "push" => last == "gpgsign" && !off,
        // `gpg.ssh.defaultKeyCommand` runs when a commit is signed with ssh.
        "gpg" => matches!(last, "program" | "defaultkeycommand"),
        "credential" => last == "helper",
        // `remote.<r>.vcs` makes git run the remote helper `git-remote-<vcs>`.
        "remote" => matches!(last, "uploadpack" | "receivepack" | "vcs"),
        "protocol" => last == "allow",
        "log" => rest == "showsignature" && !off,
        "sequence" => rest == "editor",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::runs_program;

    #[test]
    fn program_keys() {
        for (k, v) in [
            ("filter.lfs.clean", "git-lfs clean -- %f"),
            ("filter.x.process", "x"),
            ("core.fsmonitor", "true"),
            ("core.hookspath", ".githooks"),
            ("commit.gpgsign", "true"),
            ("gpg.ssh.program", "x"),
            ("diff.x.textconv", "x"),
            ("credential.https://h.helper", "store"),
            ("remote.origin.uploadpack", "x"),
            ("gpg.ssh.defaultkeycommand", "ssh-add -L"),
            ("core.gitproxy", "proxy-cmd"),
            ("remote.origin.vcs", "hg"),
        ] {
            assert!(runs_program(k, v), "{k}");
        }
        for (k, v) in [
            ("core.fsmonitor", "false"),
            ("commit.gpgsign", "false"),
            ("core.bare", "false"),
            ("user.name", "x"),
            ("remote.origin.url", "x"),
            ("filter.x.required", "true"),
        ] {
            assert!(!runs_program(k, v), "{k}");
        }
    }
}
