//! `edit.*` commands and the built-in editing keys (cursor movement and typing, which the
//! plan keeps out of the keymap in M1).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vr_core::{Selection, edit_ops, motion};

use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::{App, Focus};

macro_rules! cmd {
    ($name:literal, $title:literal, $run:expr) => {
        CommandSpec {
            name: $name,
            title: $title,
            run: $run,
        }
    };
}

pub const COMMANDS: &[CommandSpec] = &[
    cmd!("edit.undo", "Undo", undo),
    cmd!("edit.redo", "Redo", redo),
    cmd!("edit.copy", "Copy", copy),
    cmd!("edit.cut", "Cut", cut),
    cmd!("edit.paste", "Paste", paste),
    cmd!("edit.select_all", "Select All", select_all),
    cmd!("edit.delete_line", "Delete Line", delete_line),
    cmd!("edit.duplicate_line", "Duplicate Line", duplicate_line),
    cmd!("edit.move_line_up", "Move Line Up", move_line_up),
    cmd!("edit.move_line_down", "Move Line Down", move_line_down),
    cmd!("edit.indent", "Indent", indent),
    cmd!("edit.outdent", "Outdent", outdent),
    cmd!("edit.toggle_comment", "Toggle Line Comment", toggle_comment),
    cmd!("editor.edit_mode", "Enter Edit Mode", edit_mode),
    cmd!("editor.read_mode", "Enter Read Mode", read_mode),
];

/// Edit mode on the active tab (D145). From the palette or a side panel the editor also
/// gets the keys, so the mode is not dropped again on the way back.
fn edit_mode(app: &mut App, _: CmdArgs) -> CmdResult {
    if app.active_doc().is_none() {
        return Ok(());
    }
    if !matches!(app.focus, Focus::Editor | Focus::FindBar) {
        app.focus = Focus::Editor;
    }
    app.enter_edit_mode();
    Ok(())
}

/// Read mode on the active tab; the selection goes too (D145).
fn read_mode(app: &mut App, _: CmdArgs) -> CmdResult {
    if app.active_doc().is_some() {
        app.enter_read_mode();
        app.cursor_moved();
    }
    Ok(())
}

fn undo(app: &mut App, _: CmdArgs) -> CmdResult {
    if let Some(doc) = app.editable_doc() {
        doc.buffer.undo();
    }
    Ok(())
}

fn redo(app: &mut App, _: CmdArgs) -> CmdResult {
    if let Some(doc) = app.editable_doc() {
        doc.buffer.redo();
    }
    Ok(())
}

/// Selected text (ranges joined by the document's line break); `None` when nothing is
/// selected.
fn selected_text(buf: &vr_core::Buffer) -> Option<String> {
    let rope = buf.text();
    let parts: Vec<String> = buf
        .selection()
        .ranges()
        .iter()
        .filter(|r| !r.is_empty())
        .map(|r| rope.slice(r.from()..r.to()).to_string())
        .collect();
    (!parts.is_empty()).then(|| parts.join(&vr_core::text::default_line_break(rope)))
}

/// The cursor's whole line including its break (copy / cut without a selection).
fn current_line(buf: &vr_core::Buffer) -> String {
    let rope = buf.text();
    let line = rope.char_to_line(buf.selection().primary().head);
    rope.line(line).to_string()
}

fn set_clipboard(app: &mut App, text: &str) -> CmdResult {
    app.clipboard
        .set(text)
        .map_err(|e| anyhow::anyhow!("clipboard ({}): {e}", app.clipboard.kind()))
}

fn copy(app: &mut App, _: CmdArgs) -> CmdResult {
    let Some(doc) = app.active_doc() else {
        return Ok(());
    };
    let text = selected_text(&doc.buffer).unwrap_or_else(|| current_line(&doc.buffer));
    set_clipboard(app, &text)
}

fn cut(app: &mut App, _: CmdArgs) -> CmdResult {
    let Some(doc) = app.editable_doc() else {
        return Ok(());
    };
    let (text, whole_line) = match selected_text(&doc.buffer) {
        Some(t) => (t, false),
        None => (current_line(&doc.buffer), true),
    };
    set_clipboard(app, &text)?;
    let Some(doc) = app.active_doc_mut() else {
        return Ok(());
    };
    if whole_line {
        edit_ops::delete_line(&mut doc.buffer);
    } else {
        doc.buffer.delete_backward();
    }
    Ok(())
}

fn paste(app: &mut App, _: CmdArgs) -> CmdResult {
    let Some(text) = app.clipboard.get() else {
        return Ok(());
    };
    insert_pasted(app, &text);
    Ok(())
}

/// `text` with every CR, LF and CRLF replaced by `brk`. Terminals send pasted lines with CR
/// and clipboards from Windows use CRLF; left alone they would mix breaks in the file.
pub fn with_line_ending(text: &str, brk: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push_str(brk);
            }
            '\n' => out.push_str(brk),
            c => out.push(c),
        }
    }
    out
}

/// Inserts pasted text as one undo step, with the document's line break.
pub fn insert_pasted(app: &mut App, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(doc) = app.editable_doc() {
        let text = with_line_ending(text, doc.line_ending.as_str());
        doc.buffer.break_undo_group();
        doc.buffer.insert_text(&text);
        doc.buffer.break_undo_group();
    }
}

fn select_all(app: &mut App, _: CmdArgs) -> CmdResult {
    if let Some(doc) = app.active_doc_mut() {
        let len = doc.buffer.text().len_chars();
        doc.buffer.set_selection(Selection::single(0, len));
    }
    app.cursor_moved();
    Ok(())
}

fn with_buffer(app: &mut App, f: impl FnOnce(&mut vr_core::Buffer)) -> CmdResult {
    if let Some(doc) = app.editable_doc() {
        f(&mut doc.buffer);
    }
    Ok(())
}

fn delete_line(app: &mut App, _: CmdArgs) -> CmdResult {
    with_buffer(app, edit_ops::delete_line)
}

fn duplicate_line(app: &mut App, _: CmdArgs) -> CmdResult {
    with_buffer(app, edit_ops::duplicate_line)
}

fn move_line_up(app: &mut App, _: CmdArgs) -> CmdResult {
    with_buffer(app, edit_ops::move_line_up)
}

fn move_line_down(app: &mut App, _: CmdArgs) -> CmdResult {
    with_buffer(app, edit_ops::move_line_down)
}

fn indent(app: &mut App, _: CmdArgs) -> CmdResult {
    with_buffer(app, edit_ops::indent)
}

fn outdent(app: &mut App, _: CmdArgs) -> CmdResult {
    with_buffer(app, edit_ops::outdent)
}

fn toggle_comment(app: &mut App, _: CmdArgs) -> CmdResult {
    let token = app
        .active_doc()
        .and_then(|d| d.path.as_deref())
        .map_or("//", comment_token);
    with_buffer(app, |b| edit_ops::toggle_comment(b, token))
}

/// Line-comment token by file extension; C-style `//` for anything unknown.
pub fn comment_token(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    match ext.as_str() {
        "py" | "sh" | "bash" | "zsh" | "fish" | "rb" | "pl" | "toml" | "yaml" | "yml" | "conf"
        | "ini" | "r" | "cmake" | "nix" | "ps1" | "env" | "gitignore" | "dockerignore" => "#",
        "sql" | "lua" | "hs" | "elm" | "ada" => "--",
        "vim" => "\"",
        "lisp" | "clj" | "el" | "scm" => ";",
        "erl" | "tex" => "%",
        _ if matches!(name, "Makefile" | "Dockerfile" | ".gitignore" | ".env") => "#",
        _ => "//",
    }
}

/// Handles the built-in editor keys; returns false for keys the keymap should see.
pub fn handle_builtin_key(app: &mut App, key: &KeyEvent) -> bool {
    if app.active_doc().is_none() {
        return false;
    }
    let m = key.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let shift = m.contains(KeyModifiers::SHIFT);
    if alt {
        return false;
    }
    match key.code {
        KeyCode::Char(c) if !ctrl => {
            if let Some(doc) = app.editable_doc() {
                doc.buffer.insert_text(c.encode_utf8(&mut [0; 4]));
            }
        }
        KeyCode::Enter if !ctrl => insert_newline(app),
        // Terminals without the kitty protocol send ctrl+i as Tab: in read mode it is the
        // edit-mode key, in edit mode it indents as before (D145).
        KeyCode::Tab if !ctrl && !shift && !app.edit_mode() => {
            app.run_command("editor.edit_mode", CmdArgs::None);
        }
        KeyCode::Tab if !ctrl && !shift => insert_tab(app),
        KeyCode::Backspace if !ctrl => {
            if let Some(doc) = app.editable_doc() {
                doc.buffer.delete_backward();
            }
        }
        KeyCode::Delete if !ctrl && !shift => {
            if let Some(doc) = app.editable_doc() {
                doc.buffer.delete_forward();
            }
        }
        KeyCode::Left if ctrl => move_with(app, shift, motion::word_left),
        KeyCode::Right if ctrl => move_with(app, shift, motion::word_right),
        KeyCode::Left => move_with(app, shift, motion::move_left),
        KeyCode::Right => move_with(app, shift, motion::move_right),
        KeyCode::Up if !ctrl => move_with(app, shift, motion::move_up),
        KeyCode::Down if !ctrl => move_with(app, shift, motion::move_down),
        KeyCode::Home if ctrl => move_with(app, shift, motion::doc_start),
        KeyCode::End if ctrl => move_with(app, shift, motion::doc_end),
        KeyCode::Home => move_with(app, shift, motion::line_start),
        KeyCode::End => move_with(app, shift, motion::line_end),
        KeyCode::PageUp if !ctrl => page(app, shift, false),
        KeyCode::PageDown if !ctrl => page(app, shift, true),
        _ => return false,
    }
    true
}

fn move_with(app: &mut App, extend: bool, f: fn(&mut vr_core::Buffer, bool)) {
    if let Some(doc) = app.active_doc_mut() {
        f(&mut doc.buffer, extend);
    }
    app.cursor_moved();
}

/// Moves the cursor and the view by one screen of lines.
fn page(app: &mut App, extend: bool, down: bool) {
    let rows = usize::from(app.layout.editor.height.max(2)) - 1;
    let Some(doc) = app.active_doc_mut() else {
        return;
    };
    let last = doc.buffer.text().len_lines().saturating_sub(1);
    for _ in 0..rows {
        if down {
            motion::move_down(&mut doc.buffer, extend);
        } else {
            motion::move_up(&mut doc.buffer, extend);
        }
    }
    if let Some(v) = app.active_view_mut() {
        v.top = if down {
            (v.top + rows).min(last)
        } else {
            v.top.saturating_sub(rows)
        };
    }
    app.cursor_moved();
}

/// Tab inserts the indent unit up to the next tab stop; with a multi-line selection it
/// indents the lines (VS Code behaviour).
fn insert_tab(app: &mut App) {
    let Some(doc) = app.editable_doc() else {
        return;
    };
    let buf = &mut doc.buffer;
    let r = buf.selection().primary();
    let rope = buf.text();
    if rope.char_to_line(r.from()) != rope.char_to_line(r.to()) {
        edit_ops::indent(buf);
        return;
    }
    let unit = buf.options().indent_unit.clone();
    if unit == "\t" {
        buf.insert_text("\t");
        return;
    }
    let tw = buf.options().tab_width.max(1);
    let line = rope.char_to_line(r.from());
    let col = vr_core::display_col(rope.line(line), r.from() - rope.line_to_char(line), tw);
    buf.insert_text(&" ".repeat(tw - col % tw));
}

/// Line break of the document plus the current line's leading whitespace (VS Code keeps
/// the indentation).
fn insert_newline(app: &mut App) {
    let Some(doc) = app.editable_doc() else {
        return;
    };
    let rope = doc.buffer.text();
    let head = doc.buffer.selection().primary().from();
    let line = rope.char_to_line(head);
    let start = rope.line_to_char(line);
    let indent: String = rope
        .slice(start..head)
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let brk = vr_core::text::default_line_break(rope);
    doc.buffer.insert_text(&format!("{brk}{indent}"));
}
