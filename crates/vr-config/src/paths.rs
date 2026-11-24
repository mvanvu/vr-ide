//! Config and state directories (XDG on Linux), overridable by env for tests and portable setups.

use std::ffi::OsString;
use std::path::PathBuf;

const APP: &str = "vr-ide";

/// `~/.config/vr-ide` (honours `XDG_CONFIG_HOME`), or `$VR_IDE_CONFIG_DIR` when set and non-empty.
pub fn config_dir() -> PathBuf {
    resolve(std::env::var_os("VR_IDE_CONFIG_DIR"), || {
        directories::BaseDirs::new().map(|b| b.config_dir().join(APP))
    })
}

/// `~/.local/state/vr-ide` (honours `XDG_STATE_HOME`), or `$VR_IDE_STATE_DIR` when set and non-empty.
pub fn state_dir() -> PathBuf {
    resolve(std::env::var_os("VR_IDE_STATE_DIR"), || {
        directories::BaseDirs::new().map(|b| match b.state_dir() {
            Some(d) => d.join(APP),
            // Non-Linux platforms have no state dir; keep state beside the local data.
            None => b.data_local_dir().join(APP).join("state"),
        })
    })
}

fn resolve(env: Option<OsString>, base: impl FnOnce() -> Option<PathBuf>) -> PathBuf {
    match env {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        // No home directory at all (bare containers): fall back to a project-local folder.
        _ => base().unwrap_or_else(|| PathBuf::from(".").join(format!(".{APP}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_wins_when_non_empty() {
        let base = || Some(PathBuf::from("/home/u/.config/vr-ide"));
        assert_eq!(
            resolve(Some("/tmp/cfg".into()), base),
            PathBuf::from("/tmp/cfg")
        );
        assert_eq!(
            resolve(Some("".into()), base),
            PathBuf::from("/home/u/.config/vr-ide")
        );
        assert_eq!(resolve(None, base), PathBuf::from("/home/u/.config/vr-ide"));
        assert_eq!(resolve(None, || None), PathBuf::from("./.vr-ide"));
    }

    #[test]
    fn default_dirs_end_with_app_name() {
        if std::env::var_os("VR_IDE_CONFIG_DIR").is_none() {
            let c = config_dir();
            assert!(
                c.ends_with(".config/vr-ide") || c.ends_with("vr-ide"),
                "{c:?}"
            );
        }
        if std::env::var_os("VR_IDE_STATE_DIR").is_none() {
            let s = state_dir();
            assert!(s.ends_with("vr-ide"), "{s:?}");
        }
    }
}
