//! `file.*` commands. Every write to a user file goes through `Document::save*`.

use vr_core::{DiskState, DocError};

use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::App;

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "file.save",
        title: "Save",
        run: save,
    },
    CommandSpec {
        name: "file.save_overwrite",
        title: "Save (overwrite)",
        run: save_overwrite,
    },
    CommandSpec {
        name: "file.save_as",
        title: "Save As",
        run: save_as,
    },
    CommandSpec {
        name: "file.close",
        title: "Close File",
        run: close,
    },
    CommandSpec {
        name: "file.open",
        title: "Open File",
        run: open,
    },
    CommandSpec {
        name: "file.reload",
        title: "Reload File from Disk",
        run: reload,
    },
];

fn save(app: &mut App, _: CmdArgs) -> CmdResult {
    save_active(app, false)
}

/// Saves the active document through `Document::save` (or `save_overwrite` when the user
/// chose to keep their version).
fn save_active(app: &mut App, overwrite: bool) -> CmdResult {
    let name = app.active_name();
    let Some(doc) = app.active_doc_mut() else {
        return Ok(());
    };
    doc.buffer.break_undo_group();
    let result = if overwrite {
        doc.save_overwrite()
    } else {
        doc.save()
    };
    match result {
        Ok(saved) => {
            if saved.other_hard_links > 0 {
                app.warn(format!(
                    "{name} had {} other hard link(s); they keep the old content",
                    saved.other_hard_links
                ));
            }
            Ok(())
        }
        Err(DocError::ChangedOnDisk) => {
            if let Some(doc) = app.active_doc_mut() {
                doc.disk_state = DiskState::ChangedOnDisk;
            }
            let keys = app.keymap.keys_for("file.save_overwrite");
            let how = if keys.is_empty() {
                "bind file.save_overwrite to a key in keymap.toml".to_string()
            } else {
                keys.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" / ")
            };
            app.warn(format!(
                "{name} changed on disk; not saved. Save (overwrite) keeps your version: {how}"
            ));
            Ok(())
        }
        Err(e) => anyhow::bail!("{name}: {e}"),
    }
}

fn save_overwrite(app: &mut App, _: CmdArgs) -> CmdResult {
    save_active(app, true)
}

/// With `CmdArgs::Path`, writes to that path (the caller has confirmed replacing an existing
/// file). The file-name prompt arrives with the dialogs in T6.
fn save_as(app: &mut App, args: CmdArgs) -> CmdResult {
    let CmdArgs::Path(target) = args else {
        app.info("Save As needs a file name; the prompt is not available yet");
        return Ok(());
    };
    let target = app.resolve(&target);
    let Some(doc) = app.active_doc_mut() else {
        return Ok(());
    };
    doc.buffer.break_undo_group();
    let saved = doc.save_as(&target)?;
    if saved.other_hard_links > 0 {
        app.warn(format!(
            "{} had {} other hard link(s); they keep the old content",
            app.display_path(&target),
            saved.other_hard_links
        ));
    }
    Ok(())
}

/// Closing a tab with unsaved changes needs a second press (the dialog comes with T6); the
/// discarded buffer is kept in the recovery directory.
fn close(app: &mut App, _: CmdArgs) -> CmdResult {
    let Some(doc) = app.active_doc() else {
        return Ok(());
    };
    if doc.is_modified() {
        let warning = format!(
            "{} has unsaved changes: {} again to close without saving",
            app.active_name(),
            app.key_hint("file.close")
        );
        if !app.confirm_twice("file.close", warning) {
            return Ok(());
        }
        app.keep_discarded(&[app.active]);
    }
    app.close_doc(app.active);
    Ok(())
}

fn open(app: &mut App, args: CmdArgs) -> CmdResult {
    match args {
        CmdArgs::Path(p) => app.open_path(&p),
        CmdArgs::None => {
            app.info(format!(
                "Open File: use go to file ({}); the file prompt is not available yet",
                app.key_hint("file.goto")
            ));
            Ok(())
        }
    }
}

/// Replaces the buffer with the file on disk as one undo step (undo brings edits back).
fn reload(app: &mut App, _: CmdArgs) -> CmdResult {
    if let Some(doc) = app.active_doc_mut() {
        doc.reload()?;
    }
    app.cursor_moved();
    Ok(())
}
