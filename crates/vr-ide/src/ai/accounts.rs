//! Accounts per agent session (D11): one config folder per account,
//! `<state_dir>/accounts/<name>/` (0700), handed to the tool through its own variable
//! (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`) in that session's environment only. The tool writes its
//! login there; vr-ide never opens, lists or copies anything inside an account folder.
//!
//! Every name goes through [`validate`] (SEC-15); every folder is checked by [`secure`] before
//! it is used (SEC-16); removal is [`remove`] (SEC-17). Which tool an account belongs to (ui-spec
//! Q1) is kept beside the folders in `<state_dir>/ai-accounts`, one `name tool` line each.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The folder of the account folders, under the state folder.
pub const ACCOUNTS_DIR: &str = "accounts";
/// The file recording which tool each account was added under.
pub const INDEX_FILE: &str = "ai-accounts";
/// The reserved name: the tool's own config, no variable.
pub const DEFAULT: &str = "default";
/// Longest account name, in characters.
pub const NAME_MAX: usize = 32;
/// The index is a few names; anything larger is not ours and is ignored.
const INDEX_MAX: u64 = 64 * 1024;

/// The variable that moves `tool`'s config folder. The map is fixed here: a tool without an
/// entry cannot take an account, and no setting can name another variable (SEC-18).
pub fn var_for(tool: &str) -> Option<&'static str> {
    match tool {
        "claude" => Some("CLAUDE_CONFIG_DIR"),
        "codex" => Some("CODEX_HOME"),
        _ => None,
    }
}

/// Why a name is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    Chars,
    Reserved,
}

impl NameError {
    /// Fixed text: the refused name itself is never echoed.
    pub fn message(self) -> &'static str {
        match self {
            NameError::Empty => "type a name",
            NameError::TooLong => "at most 32 characters",
            NameError::Chars => "use a-z 0-9 - _ only",
            NameError::Reserved => "default is reserved",
        }
    }
}

/// The one validator for account names (SEC-15): `[a-z0-9][a-z0-9_-]{0,31}`, and not `default`
/// in any case. A valid name has no `.`, `/`, `\`, space, control or non-ASCII character, so it
/// is always one plain path component.
pub fn validate(name: &str) -> Result<&str, NameError> {
    if name.eq_ignore_ascii_case(DEFAULT) {
        return Err(NameError::Reserved);
    }
    let b = name.as_bytes();
    let Some(&first) = b.first() else {
        return Err(NameError::Empty);
    };
    let plain = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    if !plain(first) || !b.iter().all(|&c| plain(c) || c == b'-' || c == b'_') {
        return Err(NameError::Chars);
    }
    if b.len() > NAME_MAX {
        return Err(NameError::TooLong);
    }
    Ok(name)
}

/// `<state>/accounts`.
pub fn accounts_dir(state: &Path) -> PathBuf {
    state.join(ACCOUNTS_DIR)
}

/// `<state>/accounts/<name>` for a valid name; the only way an account path is made.
pub fn path_of(state: &Path, name: &str) -> Result<PathBuf, NameError> {
    validate(name).map(|n| accounts_dir(state).join(n))
}

/// The account folders: entries of `accounts/` whose name is valid and that are real folders
/// (a symlink or a file is skipped), sorted. Nothing inside them is looked at.
pub fn folders(state: &Path) -> Vec<String> {
    let dir = accounts_dir(state);
    if !is_real_dir(&dir) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            validate(&name).ok()?;
            is_real_dir(&dir.join(&name)).then_some(name)
        })
        .collect();
    out.sort();
    out
}

/// The accounts added under `tool`, sorted.
pub fn for_tool(state: &Path, tool: &str) -> Vec<String> {
    let index = read_index(state);
    folders(state)
        .into_iter()
        .filter(|n| index.get(n).is_some_and(|t| t == tool))
        .collect()
}

fn is_real_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir())
}

/// `name → tool` from the index; lines with an invalid name or tool are dropped.
fn read_index(state: &Path) -> BTreeMap<String, String> {
    let mut text = String::new();
    let read = std::fs::File::open(state.join(INDEX_FILE))
        .and_then(|f| f.take(INDEX_MAX).read_to_string(&mut text));
    if read.is_err() {
        return BTreeMap::new();
    }
    text.lines()
        .filter_map(|line| {
            let (name, tool) = line.split_once(' ')?;
            validate(name).ok()?;
            var_for(tool)?;
            Some((name.to_string(), tool.to_string()))
        })
        .collect()
}

fn write_index(state: &Path, index: &BTreeMap<String, String>) -> std::io::Result<()> {
    let text: String = index.iter().map(|(n, t)| format!("{n} {t}\n")).collect();
    vr_config::write_atomic(&state.join(INDEX_FILE), text.as_bytes())
}

/// SEC-16 before every use: `dir` must be a real folder (opened with `O_NOFOLLOW`, so a symlink
/// is refused) owned by this user; group / other bits are dropped with `fchmod` on that same
/// descriptor. Nothing inside is read.
pub fn secure(dir: &Path) -> Result<(), String> {
    use rustix::fs::{Mode, OFlags};
    use rustix::io::Errno;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = rustix::fs::open(dir, flags, Mode::empty()).map_err(|e| match e {
        Errno::NOENT => "account folder missing".to_string(),
        Errno::LOOP | Errno::NOTDIR => "account folder is not a real folder".to_string(),
        e => format!("account folder unusable ({e})"),
    })?;
    let st = rustix::fs::fstat(&fd).map_err(|e| format!("account folder unusable ({e})"))?;
    if st.st_uid != rustix::process::geteuid().as_raw() {
        return Err("account folder belongs to another user".into());
    }
    if st.st_mode & 0o077 != 0 {
        rustix::fs::fchmod(&fd, Mode::RWXU)
            .map_err(|e| format!("account folder could not be made private ({e})"))?;
    }
    Ok(())
}

/// The folder `tool` may run on as `name`, checked now (SEC-15, SEC-16, SEC-18): the tool has a
/// variable, the name is valid, it was not added under another tool, `accounts/` and the folder
/// pass [`secure`]. Never created here: a missing folder is an error, not a new account.
pub fn ready(state: Option<&Path>, tool: &str, name: &str) -> Result<PathBuf, String> {
    if var_for(tool).is_none() {
        return Err(format!("{tool} cannot use accounts"));
    }
    let path = validate(name).map_err(|_| "account name invalid".to_string())?;
    let state = state.ok_or("account folder missing")?;
    let path = accounts_dir(state).join(path);
    if let Some(other) = read_index(state).get(name)
        && other != tool
    {
        return Err(format!("account belongs to {other}"));
    }
    secure(&accounts_dir(state))?;
    secure(&path)?;
    Ok(path)
}

/// The variable for a session of `tool` on `account`, for that child's environment only (never
/// `std::env::set_var`). `None` (default) adds nothing: the inherited environment stays.
pub fn env(
    state: Option<&Path>,
    tool: &str,
    account: Option<&str>,
) -> Result<Vec<(OsString, OsString)>, String> {
    let Some(name) = account else {
        return Ok(Vec::new());
    };
    let path = ready(state, tool, name).map_err(|e| format!("account {name}: {e}"))?;
    let var = var_for(tool).expect("checked by ready");
    Ok(vec![(OsString::from(var), path.into_os_string())])
}

/// Adds account `name` under `tool`: `accounts/` and `accounts/<name>/` made 0700 and checked,
/// the tool recorded. A name taken by any tool is refused (names are unique across tools); a
/// folder left without a record (by hand, or a failed write) is taken over after the checks.
pub fn create(state: &Path, tool: &str, name: &str) -> Result<PathBuf, String> {
    let name = validate(name).map_err(|e| e.message().to_string())?;
    if var_for(tool).is_none() {
        return Err(format!("{tool} cannot use accounts"));
    }
    let mut index = read_index(state);
    match index.get(name) {
        Some(other) if other != tool => return Err(format!("{name} is taken ({other})")),
        Some(_) if is_real_dir(&accounts_dir(state).join(name)) => {
            return Err(format!("{name} exists"));
        }
        _ => {}
    }
    let dir = accounts_dir(state);
    let path = dir.join(name);
    // A symlink in either place is refused by `secure` before anything is created through it.
    if std::fs::symlink_metadata(&dir).is_ok() {
        secure(&dir)?;
    }
    crate::recovery::create_private_dir(&dir).map_err(|e| format!("cannot create folder ({e})"))?;
    secure(&dir)?;
    if std::fs::symlink_metadata(&path).is_ok() {
        secure(&path)?;
    }
    crate::recovery::create_private_dir(&path)
        .map_err(|e| format!("cannot create folder ({e})"))?;
    secure(&path)?;
    index.insert(name.to_string(), tool.to_string());
    write_index(state, &index).map_err(|e| format!("cannot record the account ({e})"))?;
    Ok(path)
}

/// Deletes account `name` for good (SEC-17): the name is validated again, `accounts/` must be a
/// real folder, and `accounts/<name>` is looked at without following links — a symlink is
/// removed as a link (its target untouched), a folder with `remove_dir_all` (which does not
/// follow links inside). No trash: the token files must not survive elsewhere. Nothing outside
/// `accounts/<name>` is touched. The caller has asked and checked that no session uses it.
pub fn remove(state: &Path, name: &str) -> Result<(), String> {
    let name = validate(name).map_err(|e| e.message().to_string())?;
    let dir = accounts_dir(state);
    if !is_real_dir(&dir) {
        return Err("account folder missing".into());
    }
    let path = dir.join(name);
    let removed = match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.file_type().is_symlink() => std::fs::remove_file(&path),
        Ok(m) if m.file_type().is_dir() => std::fs::remove_dir_all(&path),
        Ok(_) => return Err("not an account folder".into()),
    };
    removed.map_err(|e| format!("cannot remove the folder ({e})"))?;
    let mut index = read_index(state);
    if index.remove(name).is_some() {
        write_index(state, &index).map_err(|e| format!("cannot update the record ({e})"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn mode(p: &Path) -> u32 {
        std::fs::symlink_metadata(p).unwrap().mode() & 0o777
    }

    /// SEC-15 table: every refused name creates nothing.
    #[test]
    fn names_are_validated_by_one_rule() {
        let d = tempfile::tempdir().unwrap();
        let long = "a".repeat(33);
        for bad in [
            "../x",
            "/etc",
            "a/b",
            ".",
            "..",
            "A B",
            "a\x1b",
            "é",
            "Default",
            "DEFAULT",
            "default",
            "",
            "-a",
            "_a",
            "a.b",
            "a\\b",
            "a\0b",
            "Work",
            " work",
            "a\u{202e}b",
            &long,
        ] {
            assert!(validate(bad).is_err(), "{bad:?}");
            assert!(path_of(d.path(), bad).is_err(), "{bad:?}");
            assert!(create(d.path(), "claude", bad).is_err(), "{bad:?}");
            assert!(remove(d.path(), bad).is_err(), "{bad:?}");
            assert!(ready(Some(d.path()), "claude", bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            std::fs::read_dir(d.path()).unwrap().count(),
            0,
            "nothing created"
        );
        for good in ["work", "alt-2", "a", "0_x", &"a".repeat(32)] {
            assert_eq!(validate(good), Ok(good));
        }
        assert_eq!(validate("Default"), Err(NameError::Reserved));
        assert_eq!(validate(&long), Err(NameError::TooLong));
        assert_eq!(
            path_of(d.path(), "work").unwrap(),
            d.path().join("accounts/work")
        );
    }

    #[test]
    fn variables_are_a_fixed_map() {
        assert_eq!(var_for("claude"), Some("CLAUDE_CONFIG_DIR"));
        assert_eq!(var_for("codex"), Some("CODEX_HOME"));
        for t in ["gemini", "aider", "cursor-agent", "LD_PRELOAD", "PATH", ""] {
            assert_eq!(var_for(t), None);
        }
        assert_eq!(env(None, "claude", None).unwrap(), vec![]);
        assert_eq!(env(None, "gemini", None).unwrap(), vec![]);
        assert!(
            env(None, "gemini", Some("work"))
                .unwrap_err()
                .contains("gemini cannot use accounts")
        );
    }

    /// SEC-16: created 0700; a wider pre-made folder is made 0700 before use; a symlink in
    /// place of the folder (or of `accounts/`) is refused and its target gets nothing.
    #[test]
    fn folders_are_private_real_and_checked_before_use() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        let p = create(state, "claude", "work").unwrap();
        assert_eq!(p, state.join("accounts/work"));
        assert_eq!(mode(&state.join("accounts")), 0o700);
        assert_eq!(mode(&p), 0o700);
        let env = env(Some(state), "claude", Some("work")).unwrap();
        assert_eq!(
            env,
            vec![(
                OsString::from("CLAUDE_CONFIG_DIR"),
                p.clone().into_os_string()
            )]
        );
        // Pre-made wide folders.
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(
            state.join("accounts"),
            std::fs::Permissions::from_mode(0o777),
        )
        .unwrap();
        ready(Some(state), "claude", "work").unwrap();
        assert_eq!(mode(&p), 0o700);
        assert_eq!(mode(&state.join("accounts")), 0o700);
        // A symlink as the account folder.
        let outside = tempfile::tempdir().unwrap();
        std::fs::set_permissions(outside.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(outside.path(), state.join("accounts/alt")).unwrap();
        assert_eq!(
            ready(Some(state), "claude", "alt").unwrap_err(),
            "account folder is not a real folder"
        );
        assert!(create(state, "claude", "alt").is_err());
        assert_eq!(mode(outside.path()), 0o755, "target untouched");
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        assert_eq!(folders(state), ["work"], "the link is not listed");
        // A symlink as `accounts/`.
        let state2 = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), state2.path().join("accounts")).unwrap();
        assert!(create(state2.path(), "claude", "work").is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        assert!(folders(state2.path()).is_empty());
        // Missing.
        assert_eq!(
            ready(Some(state), "claude", "gone").unwrap_err(),
            "account folder missing"
        );
        assert_eq!(
            ready(None, "claude", "work").unwrap_err(),
            "account folder missing"
        );
    }

    /// The listing keeps valid real folders only; accounts belong to the tool they were added
    /// under and names are unique across tools (ui-spec Q1).
    #[test]
    fn listing_and_tools() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        create(state, "claude", "work").unwrap();
        create(state, "codex", "cx").unwrap();
        assert_eq!(
            create(state, "codex", "work").unwrap_err(),
            "work is taken (claude)"
        );
        assert_eq!(create(state, "claude", "work").unwrap_err(), "work exists");
        assert!(
            create(state, "gemini", "g")
                .unwrap_err()
                .contains("cannot use")
        );
        let acc = state.join("accounts");
        std::fs::write(acc.join("file"), "x").unwrap();
        std::fs::create_dir(acc.join("Bad")).unwrap();
        std::fs::create_dir(acc.join(".hidden")).unwrap();
        std::os::unix::fs::symlink(acc.join("work"), acc.join("link")).unwrap();
        assert_eq!(folders(state), ["cx", "work"]);
        assert_eq!(for_tool(state, "claude"), ["work"]);
        assert_eq!(for_tool(state, "codex"), ["cx"]);
        assert_eq!(
            ready(Some(state), "codex", "work").unwrap_err(),
            "account belongs to claude"
        );
        // A tampered index line cannot name another variable or a bad name.
        std::fs::write(
            state.join(INDEX_FILE),
            "work claude\n../x claude\nevil LD_PRELOAD\ncx codex\n",
        )
        .unwrap();
        assert_eq!(read_index(state).len(), 2);
        // A folder without a record is taken over when added again.
        std::fs::create_dir(acc.join("orphan")).unwrap();
        assert!(for_tool(state, "claude").iter().all(|n| n != "orphan"));
        create(state, "claude", "orphan").unwrap();
        assert_eq!(for_tool(state, "claude"), ["orphan", "work"]);
    }

    /// SEC-17: only `accounts/<name>` goes, links inside are not followed, a linked account
    /// loses only the link, `accounts/` and siblings stay.
    #[test]
    fn remove_deletes_only_the_account() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        let work = create(state, "claude", "work").unwrap();
        create(state, "claude", "alt").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("keep"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path(), work.join("link")).unwrap();
        std::fs::create_dir(work.join("sub")).unwrap();
        std::fs::write(work.join("sub/.credentials.json"), "t").unwrap();
        remove(state, "work").unwrap();
        assert!(!work.exists());
        assert!(outside.path().join("keep").exists(), "outside intact");
        assert!(state.join("accounts/alt").is_dir(), "sibling intact");
        assert!(state.join("accounts").is_dir(), "accounts/ stays");
        assert_eq!(for_tool(state, "claude"), ["alt"]);
        // A symlinked account: the link goes, its target stays.
        std::os::unix::fs::symlink(outside.path(), state.join("accounts/lnk")).unwrap();
        remove(state, "lnk").unwrap();
        assert!(std::fs::symlink_metadata(state.join("accounts/lnk")).is_err());
        assert!(outside.path().join("keep").exists());
        // Crafted names.
        for bad in ["..", ".", "../alt", "default", ""] {
            assert!(remove(state, bad).is_err(), "{bad:?}");
        }
        assert!(state.join("accounts/alt").is_dir());
    }
}
