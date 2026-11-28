//! Desktop launcher: writes and removes the `.desktop` entry and icon for vr-ide.
//!
//! The pure helpers (`build_entry`, `quote_exec_arg`, `install`, `uninstall`,
//! `migrate_on_window_start`) take the data directory and the executable as parameters so
//! tests never touch the real `~/.local/share`.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Marker carried by every file this module writes; uninstall removes only files that have it.
pub const MARKER: &str = "X-VrIde-Generated=true";
const ICON_SVG: &str = include_str!("../../../assets/vr-ide.svg");
const DESKTOP_NAME: &str = "vr-ide.desktop";
const ICON_NAME: &str = "vr-ide.svg";

/// Quote one `Exec` argument per the Desktop Entry spec, including the value-level escaping.
pub fn quote_exec_arg(arg: &str) -> String {
    let mut quoted = String::from("\"");
    for c in arg.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    // Value-level escaping: a literal backslash in a string value is written `\\`.
    quoted.replace('\\', "\\\\")
}

fn escape_value(s: &str) -> String {
    s.replace('\\', "\\\\")
}

/// Check that the executable path is safe to put into an `Exec` line.
pub fn validate_exe(exe: &Path) -> Result<&str> {
    let s = exe.to_str().context("executable path is not valid UTF-8")?;
    if !exe.is_absolute() {
        bail!("executable path is not absolute: {s}");
    }
    if s.chars().any(char::is_control) {
        bail!("executable path contains a control character");
    }
    if s.ends_with(" (deleted)") {
        bail!("executable was replaced or deleted; run the installed vr-ide again");
    }
    if !fs::metadata(exe).map(|m| m.is_file()).unwrap_or(false) {
        bail!("executable is not a regular file: {s}");
    }
    Ok(s)
}

/// How the launcher starts vr-ide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryForm {
    /// `Exec=<exe> --session` with `Terminal=false`: the binary opens its own window.
    Window,
    /// The older form: GNOME Terminal hosts the TUI.
    GnomeTerminal,
    /// The older form for other desktops: the launcher's own terminal hosts the TUI.
    Terminal,
}

/// Whether this binary opens its own window when started from the launcher.
///
/// The single switch for the entry form: only a build with the window can start from a
/// `Terminal=false` entry, so a TUI-only build keeps writing a terminal form that works.
pub fn launches_in_window() -> bool {
    cfg!(feature = "window")
}

/// The form `--install-desktop` writes: the window form when this build has the window,
/// else GNOME Terminal when it is installed, else the launcher's own terminal.
pub fn entry_form(window: bool, gnome_terminal_on_path: bool) -> EntryForm {
    if window {
        EntryForm::Window
    } else if gnome_terminal_on_path {
        EntryForm::GnomeTerminal
    } else {
        EntryForm::Terminal
    }
}

/// First token of the GNOME Terminal form's `Exec`; it is how an old entry is recognised.
const GNOME_EXEC_PREFIX: &str = "gnome-terminal --title=vr-ide -- ";

/// Text of the `.desktop` entry in the given form.
pub fn build_entry(exe: &str, form: EntryForm) -> String {
    let exe_q = quote_exec_arg(exe);
    let (exec, terminal, wm_class) = match form {
        EntryForm::Window => (
            format!("{exe_q} --session"),
            "false",
            "StartupWMClass=vr-ide\n",
        ),
        EntryForm::GnomeTerminal => (format!("{GNOME_EXEC_PREFIX}{exe_q} --session"), "false", ""),
        EntryForm::Terminal => (format!("{exe_q} --session"), "true", ""),
    };
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=vr-ide\n\
         Comment=Terminal IDE\n\
         Exec={exec}\n\
         TryExec={try_exec}\n\
         Icon=vr-ide\n\
         Terminal={terminal}\n\
         {wm_class}\
         Categories=Development;IDE;\n\
         {MARKER}\n",
        try_exec = escape_value(exe),
    )
}

fn desktop_path(data_home: &Path) -> PathBuf {
    data_home.join("applications").join(DESKTOP_NAME)
}

fn icon_path(data_home: &Path) -> PathBuf {
    data_home
        .join("icons/hicolor/scalable/apps")
        .join(ICON_NAME)
}

/// Write `contents` next to `path` and rename over it; a symlink at `path` is replaced, not followed.
fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let dir = path.parent().context("path has no parent")?;
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let tmp = dir.join(format!(".vr-ide-{}.tmp", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(&tmp)
        .with_context(|| format!("create {}", tmp.display()))?;
    let res = f
        .write_all(contents.as_bytes())
        .and_then(|()| f.sync_all())
        .and_then(|()| fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644)))
        .and_then(|()| fs::rename(&tmp, path));
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res.with_context(|| format!("write {}", path.display()))
}

const SVG_MARKER: &str = "<!-- X-VrIde-Generated=true -->";

/// True for a regular file (never a symlink) that vr-ide wrote: the `.desktop` carries the
/// marker line, the icon carries the marker comment line or equals the bundled icon.
fn is_ours(path: &Path) -> bool {
    let Some(content) = read_regular_nofollow(path) else {
        return false;
    };
    if path.extension().is_some_and(|e| e == "svg") {
        content == ICON_SVG || content.lines().any(|l| l.trim() == SVG_MARKER)
    } else {
        content.lines().any(|l| l.trim() == MARKER)
    }
}

/// Refuse a target that exists and is not ours; an absent one is fine.
fn check_target(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) if is_ours(path) => Ok(true),
        Ok(_) => bail!(
            "{} exists and was not written by vr-ide; remove it first",
            path.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("check {}", path.display())),
    }
}

/// Install the entry and icon below `data_home`; returns the written paths. Both targets are
/// checked before anything is written, and a failed second write undoes the first.
pub fn install(data_home: &Path, exe: &Path, form: EntryForm) -> Result<Vec<PathBuf>> {
    let exe = validate_exe(exe)?;
    let desktop = desktop_path(data_home);
    let icon = icon_path(data_home);
    check_target(&desktop)?;
    let icon_existed = check_target(&icon)?;
    write_atomic(&icon, ICON_SVG)?;
    if let Err(e) = write_atomic(&desktop, &build_entry(exe, form)) {
        if !icon_existed {
            let _ = fs::remove_file(&icon);
        }
        return Err(e);
    }
    Ok(vec![desktop, icon])
}

/// The `TryExec` path of an entry, with the value-level escaping undone.
fn try_exec_of(entry: &str) -> Option<PathBuf> {
    let raw = entry.lines().find_map(|l| l.strip_prefix("TryExec="))?;
    let mut out = String::new();
    let mut it = raw.chars();
    while let Some(c) = it.next() {
        out.push(if c == '\\' { it.next()? } else { c });
    }
    Some(PathBuf::from(out))
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// On a window start, rewrite an entry vr-ide wrote in a terminal form (GNOME Terminal, or
/// `Terminal=true` with `Exec=<exe> --session`) into the window form.
/// Returns a notice for the user when it rewrote the entry, a warning when the rewrite failed,
/// and `None` when there was nothing to do.
///
/// Only a regular file (never a symlink) carrying the marker line and naming this same binary
/// in `TryExec` is rewritten: an entry for another install (say the packaged binary while a
/// development build runs) is left alone, so the menu never starts to point at a build that may
/// go away. The write keeps `install`'s guarantees (no symlink following, atomic rename).
pub fn migrate_on_window_start(data_home: &Path, exe: &Path) -> Option<String> {
    let desktop = desktop_path(data_home);
    let content = read_regular_nofollow(&desktop)?;
    if !content.lines().any(|l| l.trim() == MARKER) {
        return None;
    }
    let exec = content.lines().find_map(|l| l.strip_prefix("Exec="))?;
    let try_exec = try_exec_of(&content)?;
    if !same_file(&try_exec, exe) {
        return None;
    }
    let gnome_form = exec.starts_with(GNOME_EXEC_PREFIX);
    // Exactly what `build_entry` wrote for the terminal form, so a hand-edited Exec is kept.
    let terminal_form = content.lines().any(|l| l == "Terminal=true")
        && try_exec
            .to_str()
            .is_some_and(|t| exec == format!("{} --session", quote_exec_arg(t)));
    if !gnome_form && !terminal_form {
        return None;
    }
    let res = validate_exe(exe).and_then(|exe_s| {
        // Re-checked right before the write: the file must still be ours.
        check_target(&desktop)?;
        write_atomic(&desktop, &build_entry(exe_s, EntryForm::Window))
    });
    match res {
        // No database refresh: the entry has no `MimeType`, so the MIME cache is unchanged.
        Ok(()) => Some("Updated the vr-ide menu entry to open its own window.".into()),
        Err(e) => Some(format!(
            "warning: could not update the vr-ide menu entry: {e:#}"
        )),
    }
}

/// A note for the user when the entry points at a binary other than the `vr-ide` on `PATH`.
pub fn path_note(exe: &Path, on_path: Option<&Path>) -> Option<String> {
    let same = on_path.is_some_and(|p| match (p.canonicalize(), exe.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => p == exe,
    });
    (!same).then(|| {
        format!(
            "note: the entry starts {}, not a vr-ide found on PATH",
            exe.display()
        )
    })
}

fn read_regular_nofollow(path: &Path) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path)
        .ok()?;
    let mut s = String::new();
    File::read_to_string(&mut f, &mut s).ok()?;
    Some(s)
}

/// Remove the two files when they carry the marker; returns what was removed.
pub fn uninstall(data_home: &Path) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for path in [desktop_path(data_home), icon_path(data_home)] {
        let ours = is_ours(&path);
        if ours {
            fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
            removed.push(path);
        }
    }
    Ok(removed)
}

/// `$XDG_DATA_HOME` when absolute, else `$HOME/.local/share`.
pub fn data_home() -> Result<PathBuf> {
    if let Some(v) = std::env::var_os("XDG_DATA_HOME") {
        let p = PathBuf::from(v);
        if p.is_absolute() {
            return Ok(p);
        }
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local/share"))
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    find_in(std::env::var_os("PATH").as_deref(), name)
}

/// The program `name` in an absolute entry of `path`; a relative entry never supplies it.
pub(crate) fn find_in(path: Option<&std::ffi::OsStr>, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|d| d.is_absolute())
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

fn refresh_database(data_home: &Path) {
    refresh_database_with(data_home, std::env::var_os("PATH").as_deref());
}

/// Runs the resolved absolute path, never the bare name: the child would search `PATH` again.
pub(crate) fn refresh_database_with(data_home: &Path, path: Option<&std::ffi::OsStr>) {
    if let Some(prog) = find_in(path, "update-desktop-database") {
        let _ = std::process::Command::new(prog)
            .arg(data_home.join("applications"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// `--install-desktop`: returns a message for the user.
pub fn run_install() -> Result<String> {
    let exe = std::env::current_exe().context("cannot find the vr-ide executable")?;
    let home = data_home()?;
    let form = entry_form(
        launches_in_window(),
        find_in_path("gnome-terminal").is_some(),
    );
    let written = install(&home, &exe, form)?;
    refresh_database(&home);
    let mut lines: Vec<String> = written
        .iter()
        .map(|p| format!("wrote {}", p.display()))
        .collect();
    lines.extend(path_note(&exe, find_in_path("vr-ide").as_deref()));
    Ok(lines.join("\n"))
}

/// `--uninstall-desktop`: returns a message for the user.
pub fn run_uninstall() -> Result<String> {
    let home = data_home()?;
    let removed = uninstall(&home)?;
    refresh_database(&home);
    if removed.is_empty() {
        return Ok("nothing to remove".into());
    }
    Ok(removed
        .iter()
        .map(|p| format!("removed {}", p.display()))
        .collect::<Vec<_>>()
        .join("\n"))
}
