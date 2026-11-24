//! Reusable modal widgets: a confirm dialog (message + buttons) and a prompt dialog (one
//! text input). They know nothing about the app; the owner turns an [`Outcome`] into an action.
//!
//! Built-in keys (like the palette's, not in the keymap): Left/Right/Tab/Shift+Tab move between
//! buttons, Enter chooses or submits, Esc cancels, the usual line-editing keys edit the input.

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Span;

use crate::theme::Theme;

/// What a key, click or paste did to a dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Still open (moved, typed, ignored).
    Pending,
    /// A confirm dialog's button, by index.
    Chosen(usize),
    /// A prompt dialog's text.
    Submitted(String),
    /// Esc (or the Cancel role).
    Cancelled,
}

/// One-line text input with a char cursor. Shared by the prompt dialog and the palette.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineInput {
    text: String,
    /// Cursor as a char index into `text`.
    cursor: usize,
}

impl LineInput {
    /// Input holding `text` with the cursor at its end.
    pub fn new(text: &str) -> LineInput {
        LineInput {
            text: text.to_string(),
            cursor: text.chars().count(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Replaces the text and puts the cursor at its end.
    pub fn set(&mut self, text: &str) {
        *self = LineInput::new(text);
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map_or(self.text.len(), |(b, _)| b)
    }

    /// Cursor steps by grapheme cluster, so a decomposed "ệ" is one step and one delete.
    fn step(&self, forward: bool) -> usize {
        let rope = ropey::Rope::from_str(&self.text);
        if forward {
            vr_core::text::next_grapheme_boundary(rope.slice(..), self.cursor)
        } else {
            vr_core::text::prev_grapheme_boundary(rope.slice(..), self.cursor)
        }
    }

    fn remove_chars(&mut self, from: usize, to: usize) {
        let (a, b) = (self.byte_at(from), self.byte_at(to));
        self.text.replace_range(a..b, "");
        self.cursor = from;
    }

    /// Editing keys (typing, Backspace, Delete, Left, Right, Home, End); true when the key
    /// was one of them. Keys with ctrl or alt are left to the caller.
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return false;
        }
        match key.code {
            KeyCode::Char(c) => self.insert_str(c.encode_utf8(&mut [0; 4])),
            KeyCode::Backspace if self.cursor > 0 => {
                let from = self.step(false);
                self.remove_chars(from, self.cursor);
            }
            KeyCode::Delete => {
                let to = self.step(true);
                self.remove_chars(self.cursor, to);
            }
            KeyCode::Left => self.cursor = self.step(false),
            KeyCode::Right => self.cursor = self.step(true),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.chars().count(),
            KeyCode::Backspace => {}
            _ => return false,
        }
        true
    }

    /// Inserts pasted text at the cursor; line breaks and other control characters become
    /// spaces (a one-line input).
    pub fn insert_str(&mut self, s: &str) {
        let clean: String = s
            .replace("\r\n", "\n")
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let at = self.byte_at(self.cursor);
        self.text.insert_str(at, &clean);
        self.cursor += clean.chars().count();
    }

    /// The part of the text that fits in `width` cells with the cursor visible, and the
    /// cursor's cell offset in it.
    pub fn view(&self, width: u16) -> (String, u16) {
        let width = usize::from(width.max(1));
        let chars: Vec<char> = self.text.chars().collect();
        let cell = |c: &char| Span::raw(c.to_string()).width();
        // Drop chars from the left until the cursor (plus one cell for it) fits.
        let mut start = 0;
        while start < self.cursor
            && chars[start..self.cursor].iter().map(cell).sum::<usize>() + 1 > width
        {
            start += 1;
        }
        let mut out = String::new();
        let mut used = 0;
        for c in &chars[start..] {
            let w = cell(c);
            if used + w > width {
                break;
            }
            used += w;
            out.push(*c);
        }
        let x: usize = chars[start..self.cursor].iter().map(cell).sum();
        (out, x as u16)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmDialog {
    pub title: String,
    pub message: String,
    pub buttons: Vec<String>,
    pub selected: usize,
    /// Screen areas of the buttons in the last frame (mouse hit test).
    pub button_areas: Vec<Rect>,
}

impl ConfirmDialog {
    pub fn new(title: impl Into<String>, message: impl Into<String>, buttons: &[&str]) -> Self {
        ConfirmDialog {
            title: title.into(),
            message: message.into(),
            buttons: buttons.iter().map(|b| (*b).to_string()).collect(),
            selected: 0,
            button_areas: Vec::new(),
        }
    }

    /// Left/Right/Tab/Shift+Tab move (wrapping), Enter chooses, Esc cancels. Auto-repeat of a
    /// held Enter never chooses: the key that opened the dialog may still be held down.
    pub fn handle_key(&mut self, key: &KeyEvent) -> Outcome {
        let n = self.buttons.len().max(1);
        match key.code {
            KeyCode::Esc => return Outcome::Cancelled,
            KeyCode::Enter if key.kind == KeyEventKind::Repeat => {}
            KeyCode::Enter if !self.buttons.is_empty() => return Outcome::Chosen(self.selected),
            KeyCode::Right | KeyCode::Tab if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.selected = (self.selected + 1) % n;
            }
            KeyCode::Left | KeyCode::BackTab | KeyCode::Tab => {
                self.selected = (self.selected + n - 1) % n;
            }
            _ => {}
        }
        Outcome::Pending
    }

    /// A left click on a button (as drawn in the last frame) chooses it.
    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> Outcome {
        if ev.kind != MouseEventKind::Down(MouseButton::Left) {
            return Outcome::Pending;
        }
        match self
            .button_areas
            .iter()
            .position(|r| contains(*r, ev.column, ev.row))
        {
            Some(i) => {
                self.selected = i;
                Outcome::Chosen(i)
            }
            None => Outcome::Pending,
        }
    }
}

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// Word wrap by display width; a word longer than `width` is cut by chars.
pub fn wrap(text: &str, width: u16) -> Vec<String> {
    let width = usize::from(width.max(1));
    let w = |s: &str| Span::raw(s.to_string()).width();
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let sep = usize::from(!line.is_empty());
            if w(&line) + sep + w(word) <= width {
                if sep == 1 {
                    line.push(' ');
                }
                line.push_str(word);
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            for c in word.chars() {
                if w(&line) + w(c.encode_utf8(&mut [0; 4])) > width {
                    lines.push(std::mem::take(&mut line));
                }
                line.push(c);
            }
        }
        lines.push(line);
    }
    lines
}

/// Button labels as drawn ("[Save all]").
fn button_label(b: &str) -> String {
    format!("[{b}]")
}

/// Rows of buttons (indices) packed into `width` cells, one space apart.
fn button_rows(buttons: &[String], width: u16) -> Vec<Vec<usize>> {
    let width = usize::from(width);
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut used = 0;
    for (i, b) in buttons.iter().enumerate() {
        let bw = Span::raw(button_label(b)).width();
        match rows.last_mut() {
            Some(row) if used + 1 + bw <= width => {
                row.push(i);
                used += 1 + bw;
            }
            _ => {
                rows.push(vec![i]);
                used = bw;
            }
        }
    }
    rows
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptDialog {
    pub title: String,
    pub label: String,
    pub input: LineInput,
    /// Shown under the input until the next edit (e.g. "no such line").
    pub error: Option<String>,
}

impl PromptDialog {
    pub fn new(title: impl Into<String>, label: impl Into<String>, initial: &str) -> Self {
        PromptDialog {
            title: title.into(),
            label: label.into(),
            input: LineInput::new(initial),
            error: None,
        }
    }

    /// Enter submits (not on auto-repeat), Esc cancels, editing keys edit and clear the error.
    pub fn handle_key(&mut self, key: &KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Esc => Outcome::Cancelled,
            KeyCode::Enter if key.kind == KeyEventKind::Repeat => Outcome::Pending,
            KeyCode::Enter => Outcome::Submitted(self.input.text().to_string()),
            _ => {
                if self.input.handle_key(key) {
                    self.error = None;
                }
                Outcome::Pending
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogWidget {
    Confirm(ConfirmDialog),
    Prompt(PromptDialog),
}

impl DialogWidget {
    pub fn handle_key(&mut self, key: &KeyEvent) -> Outcome {
        match self {
            DialogWidget::Confirm(d) => d.handle_key(key),
            DialogWidget::Prompt(d) => d.handle_key(key),
        }
    }

    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> Outcome {
        match self {
            DialogWidget::Confirm(d) => d.handle_mouse(ev),
            DialogWidget::Prompt(_) => Outcome::Pending,
        }
    }

    pub fn handle_paste(&mut self, text: &str) {
        if let DialogWidget::Prompt(d) = self {
            d.input.insert_str(text);
            d.error = None;
        }
    }

    /// Draws the dialog centred in `area`.
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if area.width < 8 || area.height < 3 {
            return;
        }
        // Inner text width: as wide as the content needs, at most 60 cells, never wider
        // than the screen minus the borders and one cell of padding each side.
        let max_inner = area.width.saturating_sub(4).clamp(1, 60);
        let (title, wanted) = match &*self {
            DialogWidget::Confirm(d) => {
                let buttons: usize = d
                    .buttons
                    .iter()
                    .map(|b| Span::raw(button_label(b)).width() + 1)
                    .sum();
                let msg = d
                    .message
                    .split('\n')
                    .map(|l| Span::raw(l.to_string()).width())
                    .max()
                    .unwrap_or(0);
                (d.title.clone(), msg.max(buttons))
            }
            DialogWidget::Prompt(d) => (d.title.clone(), 48),
        };
        let title_w = Span::raw(title.clone()).width() + 2;
        let inner_w = (wanted.max(title_w).min(usize::from(u16::MAX)) as u16).clamp(1, max_inner);

        enum Row {
            Text(String, ratatui::style::Style),
            Input,
            Buttons(Vec<usize>),
        }
        let mut rows: Vec<Row> = Vec::new();
        match &*self {
            DialogWidget::Confirm(d) => {
                for l in wrap(&d.message, inner_w) {
                    rows.push(Row::Text(l, ratatui::style::Style::default()));
                }
                rows.push(Row::Text(String::new(), ratatui::style::Style::default()));
                for r in button_rows(&d.buttons, inner_w) {
                    rows.push(Row::Buttons(r));
                }
            }
            DialogWidget::Prompt(d) => {
                for l in wrap(&d.label, inner_w) {
                    rows.push(Row::Text(l, ratatui::style::Style::default()));
                }
                rows.push(Row::Input);
                if let Some(e) = &d.error {
                    for l in wrap(e, inner_w) {
                        rows.push(Row::Text(l, theme.error));
                    }
                }
                for l in wrap("Enter: OK · Esc: Cancel", inner_w) {
                    rows.push(Row::Text(l, theme.dim));
                }
            }
        }
        // On a short screen the input and the buttons stay: message rows go first.
        let max_rows = usize::from(area.height.saturating_sub(2)).max(1);
        while rows.len() > max_rows {
            match rows.iter().position(|r| matches!(r, Row::Text(..))) {
                Some(i) => {
                    rows.remove(i);
                }
                None => rows.truncate(max_rows),
            }
        }

        let w = inner_w + 4;
        let h = rows.len() as u16 + 2;
        let rect = Rect::new(
            area.x + (area.width - w) / 2,
            area.y + (area.height - h) / 2,
            w,
            h,
        );
        let buf = frame.buffer_mut();
        ratatui::widgets::Widget::render(ratatui::widgets::Clear, rect, buf);
        let block = ratatui::widgets::Block::bordered()
            .title(format!(" {title} "))
            .style(theme.popup)
            .border_style(theme.popup_border);
        ratatui::widgets::Widget::render(block, rect, buf);

        let x0 = rect.x + 2;
        let mut cursor = None;
        let mut button_areas = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let y = rect.y + 1 + i as u16;
            match row {
                Row::Text(t, style) => {
                    buf.set_stringn(x0, y, t, usize::from(inner_w), theme.popup.patch(*style));
                }
                Row::Input => {
                    if let DialogWidget::Prompt(d) = &*self {
                        let (shown, cx) = d.input.view(inner_w);
                        buf.set_style(Rect::new(x0, y, inner_w, 1), theme.input);
                        buf.set_stringn(x0, y, shown, usize::from(inner_w), theme.input);
                        cursor = Some((x0 + cx.min(inner_w - 1), y));
                    }
                }
                Row::Buttons(idxs) => {
                    let DialogWidget::Confirm(d) = &*self else {
                        continue;
                    };
                    let mut x = x0;
                    for &b in idxs {
                        let label = button_label(&d.buttons[b]);
                        let style = if b == d.selected {
                            theme.button_selected
                        } else {
                            theme.button
                        };
                        let bw = (Span::raw(label.clone()).width() as u16).min(x0 + inner_w - x);
                        buf.set_stringn(x, y, &label, usize::from(bw), style);
                        button_areas.push((b, Rect::new(x, y, bw, 1)));
                        x += bw + 1;
                    }
                }
            }
        }
        if let DialogWidget::Confirm(d) = self {
            button_areas.sort_by_key(|(b, _)| *b);
            d.button_areas = button_areas.into_iter().map(|(_, r)| r).collect();
        }
        if let Some(pos) = cursor {
            frame.set_cursor_position(pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn line_input_edits_at_the_cursor() {
        let mut i = LineInput::new("ab");
        assert_eq!((i.text(), i.cursor()), ("ab", 2));
        i.handle_key(&k(KeyCode::Left));
        i.handle_key(&k(KeyCode::Char('ệ')));
        assert_eq!(i.text(), "aệb");
        i.handle_key(&k(KeyCode::Home));
        i.handle_key(&k(KeyCode::Delete));
        i.handle_key(&k(KeyCode::End));
        i.handle_key(&k(KeyCode::Backspace));
        assert_eq!((i.text(), i.cursor()), ("ệ", 1));
        i.insert_str("x\ny");
        assert_eq!(i.text(), "ệx y");
        assert!(!i.handle_key(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)));
    }

    fn draw(
        w: &mut DialogWidget,
        cols: u16,
        rows: u16,
    ) -> ratatui::Terminal<ratatui::backend::TestBackend> {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(cols, rows)).unwrap();
        t.draw(|f| w.render(f, f.area(), &Theme::dark())).unwrap();
        t
    }

    #[test]
    fn confirm_dialog_keys_and_mouse() {
        let d = ConfirmDialog::new(
            "Quit",
            "2 unsaved files",
            &["Save all", "Discard", "Cancel"],
        );
        let mut w = DialogWidget::Confirm(d.clone());
        assert_eq!(w.handle_key(&k(KeyCode::Right)), Outcome::Pending);
        assert_eq!(w.handle_key(&k(KeyCode::Tab)), Outcome::Pending);
        assert_eq!(w.handle_key(&k(KeyCode::Tab)), Outcome::Pending);
        assert_eq!(
            w.handle_key(&k(KeyCode::Enter)),
            Outcome::Chosen(0),
            "wraps around"
        );
        w.handle_key(&k(KeyCode::Left));
        w.handle_key(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        // A held Enter (auto-repeat) never chooses.
        let mut held = k(KeyCode::Enter);
        held.kind = KeyEventKind::Repeat;
        assert_eq!(w.handle_key(&held), Outcome::Pending);
        assert_eq!(w.handle_key(&k(KeyCode::Enter)), Outcome::Chosen(1));
        assert_eq!(w.handle_key(&k(KeyCode::Esc)), Outcome::Cancelled);
        // Mouse: a click on a drawn button chooses it, at both sizes.
        for (cols, rows) in [(80, 24), (40, 12)] {
            let mut w = DialogWidget::Confirm(d.clone());
            let t = draw(&mut w, cols, rows);
            let DialogWidget::Confirm(c) = &w else {
                unreachable!()
            };
            assert_eq!(c.button_areas.len(), 3);
            let b = c.button_areas[2];
            let row: String = (b.x..b.x + b.width)
                .map(|x| t.backend().buffer()[(x, b.y)].symbol().to_string())
                .collect();
            assert!(row.contains("Cancel"), "{row:?}");
            let click = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: b.x + 1,
                row: b.y,
                modifiers: KeyModifiers::NONE,
            };
            assert_eq!(w.handle_mouse(&click), Outcome::Chosen(2));
            let miss = MouseEvent {
                column: 0,
                row: 0,
                ..click
            };
            assert_eq!(w.handle_mouse(&miss), Outcome::Pending);
        }
    }

    #[test]
    fn prompt_dialog_submits_and_cancels() {
        let mut w = DialogWidget::Prompt(PromptDialog::new("Go to Line", "line[:col]", ""));
        for c in "12:4".chars() {
            assert_eq!(w.handle_key(&k(KeyCode::Char(c))), Outcome::Pending);
        }
        w.handle_paste("0");
        assert_eq!(
            w.handle_key(&k(KeyCode::Enter)),
            Outcome::Submitted("12:40".into())
        );
        if let DialogWidget::Prompt(p) = &mut w {
            p.error = Some("no such line".into());
        }
        w.handle_key(&k(KeyCode::Backspace));
        let DialogWidget::Prompt(p) = &w else {
            unreachable!()
        };
        assert_eq!(
            (p.input.text(), p.error.clone()),
            ("12:4", None),
            "an edit clears the error"
        );
        let t = draw(&mut w, 40, 12);
        let screen: String = t
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            screen.contains("12:4") && screen.contains("Go to Line"),
            "{screen}"
        );
        assert_eq!(w.handle_key(&k(KeyCode::Esc)), Outcome::Cancelled);
    }
}
