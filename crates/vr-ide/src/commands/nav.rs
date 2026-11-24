//! `nav.*` commands.

use vr_core::{Pos, Selection};

use super::dialog::DialogAction;
use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::App;
use crate::ui::dialog::{DialogWidget, PromptDialog};

pub const COMMANDS: &[CommandSpec] = &[CommandSpec {
    name: "nav.goto_line",
    title: "Go to Line",
    run: goto_line,
}];

/// Asks for "line[:col]".
fn goto_line(app: &mut App, _: CmdArgs) -> CmdResult {
    let Some(doc) = app.active_doc() else {
        app.info("Go to Line: no file is open");
        return Ok(());
    };
    let lines = doc.buffer.text().len_lines();
    let label = format!("Line (1-{lines}), optionally :column");
    app.open_dialog(
        DialogWidget::Prompt(PromptDialog::new("Go to Line", label, "")),
        DialogAction::GotoLine,
    );
    Ok(())
}

/// Moves the cursor to `input` ("line" or "line:col", 1-based, clamped to the document and
/// to the line's text). `Err` holds the message for the prompt.
pub fn goto(app: &mut App, input: &str) -> Result<(), String> {
    const HINT: &str = "type a line number, e.g. 42 or 42:7";
    let input = input.trim();
    let (line, col) = match input.split_once(':') {
        Some((l, c)) => (l.trim(), Some(c.trim())),
        None => (input, None),
    };
    let line: usize = line.parse().map_err(|_| HINT.to_string())?;
    let col: usize = match col {
        Some(c) => c.parse().map_err(|_| HINT.to_string())?,
        None => 1,
    };
    let Some(doc) = app.active_doc_mut() else {
        return Ok(());
    };
    let rope = doc.buffer.text();
    let last = rope.len_lines().saturating_sub(1);
    let at = vr_core::text::pos_to_char(
        rope,
        Pos {
            line: line.saturating_sub(1).min(last),
            col: col.saturating_sub(1),
        },
    );
    doc.buffer.set_selection(Selection::point(at));
    app.cursor_moved();
    Ok(())
}
