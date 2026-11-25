//! The explorer's right-click menu (T23, D262): the explorer key actions for the row under
//! the pointer. An item only runs the `explorer.*` command its key runs, on the row the
//! right click selected, so the menu and the keys can never act differently. Drawn by
//! `ui::explorer_menu`.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::text::Span;

use super::registry::CmdArgs;
use crate::app::App;

/// Label and command of each item, in menu order.
const ITEMS: [(&str, &str); 6] = [
    ("New…", "explorer.new"),
    ("Rename…", "explorer.rename"),
    ("Copy", "explorer.copy"),
    ("Cut", "explorer.cut"),
    ("Paste", "explorer.paste"),
    ("Delete", "explorer.delete"),
];

/// The extra item of a root row (W-2): runs `workspace.remove_folder` on that root; files on
/// disk are never touched.
pub const REMOVE_ROOT: &str = "Remove Folder from Workspace";

/// Cells between the longest label and the key column.
const KEY_GAP: u16 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: &'static str,
    pub command: &'static str,
    /// The command's key as the keymap has it (empty when unbound).
    pub key: String,
    /// False: drawn dimmed, skipped by the arrows, a click does nothing.
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub items: Vec<Item>,
    /// Highlighted item; always an enabled one when any is.
    pub selected: usize,
    /// Pointer cell of the right click (the menu's preferred top-left corner).
    pub at: (u16, u16),
    /// Box on screen, borders included: placed at open and again each frame (same result
    /// for the same screen), so clicks hit what is drawn.
    pub area: Rect,
}

impl Menu {
    /// Width and height of the box, borders included.
    fn size(&self) -> (u16, u16) {
        let w = |s: &str| Span::raw(s).width() as u16;
        let label = self.items.iter().map(|i| w(i.label)).max().unwrap_or(0);
        let key = self.items.iter().map(|i| w(&i.key)).max().unwrap_or(0);
        // Border, a space, label, gap, key, a space, border.
        (label + KEY_GAP + key + 4, self.items.len() as u16 + 2)
    }

    /// Places the box just under the pointer's row (that row stays readable), or just above
    /// it when there is no room below; moved left / up as needed to stay inside `screen`.
    pub fn place(&mut self, screen: Rect) {
        let (w, h) = self.size();
        let (w, h) = (w.min(screen.width), h.min(screen.height));
        let right = screen.x + screen.width;
        let bottom = screen.y + screen.height;
        let (col, row) = self.at;
        let x = col.clamp(screen.x, right - w);
        let y = if row + 1 + h <= bottom {
            row + 1
        } else if row >= screen.y + h {
            row - h
        } else {
            bottom - h
        };
        self.area = Rect::new(x, y.max(screen.y), w, h);
    }

    /// Item index under the screen cell, when it is drawn.
    fn item_at(&self, column: u16, row: u16) -> Option<usize> {
        let a = self.area;
        let inside =
            column > a.x && column + 1 < a.x + a.width && row > a.y && row + 1 < a.y + a.height;
        inside.then(|| usize::from(row - a.y - 1))
    }

    fn contains(&self, column: u16, row: u16) -> bool {
        let a = self.area;
        column >= a.x && column < a.x + a.width && row >= a.y && row < a.y + a.height
    }

    /// Next enabled item from `selected` in direction `step` (stays when there is none).
    fn step(&mut self, step: isize) {
        let mut i = self.selected as isize;
        loop {
            i += step;
            if i < 0 || i as usize >= self.items.len() {
                return;
            }
            if self.items[i as usize].enabled {
                self.selected = i as usize;
                return;
            }
        }
    }
}

/// The screen as the last frame laid it out (the status row spans its whole width and is
/// its last row).
fn screen(app: &App) -> Rect {
    let l = app.layout;
    let width = [l.status, l.explorer, l.editor, l.tabs, l.terminal]
        .iter()
        .map(|r| r.x + r.width)
        .max()
        .unwrap_or(0);
    let height = [l.status, l.explorer, l.editor, l.notice, l.terminal]
        .iter()
        .map(|r| r.y + r.height)
        .max()
        .unwrap_or(0);
    Rect::new(0, 0, width, height)
}

/// Opens the menu at the pointer cell for the explorer's current row and selection (the
/// caller selected them). Actions the target refuses are dimmed: the root row is never
/// renamed, copied, cut or deleted; Paste needs something copied or cut.
pub fn open(app: &mut App, column: u16, row: u16) {
    let ex = &app.explorer;
    let rename = ex.current().is_some_and(|r| r.depth > 0);
    let (paths, _) = super::explorer::targets(app);
    let paste = !ex.copied.is_empty();
    // A root row of a workspace with several roots also takes that root out of the
    // workspace (W-2); the last root stays (D241), so one root never offers it.
    let remove_root = ex.is_multi() && ex.current().is_some_and(|r| r.depth == 0);
    let mut items: Vec<Item> = ITEMS
        .iter()
        .map(|&(label, command)| Item {
            label,
            command,
            key: app.display_key(command).unwrap_or_default(),
            enabled: match command {
                "explorer.new" => true,
                "explorer.rename" => rename,
                "explorer.paste" => paste,
                _ => !paths.is_empty(),
            },
        })
        .collect();
    if remove_root {
        items.push(Item {
            label: REMOVE_ROOT,
            command: "workspace.remove_folder",
            key: app
                .display_key("workspace.remove_folder")
                .unwrap_or_default(),
            enabled: true,
        });
    }
    let selected = items.iter().position(|i| i.enabled).unwrap_or(0);
    let mut menu = Menu {
        items,
        selected,
        at: (column, row),
        area: Rect::default(),
    };
    menu.place(screen(app));
    app.explorer.menu = Some(menu);
    // Enter right after the menu appears was typed for something else (as for dialogs).
    app.modal_since = Some(app.now());
    app.dirty = true;
}

pub fn close(app: &mut App) {
    if app.explorer.menu.take().is_some() {
        app.dirty = true;
    }
}

/// Runs item `i` when it is enabled: the menu closes, then its command runs as its key would.
fn choose(app: &mut App, i: usize) {
    let Some(item) = app.explorer.menu.as_ref().and_then(|m| m.items.get(i)) else {
        return;
    };
    if !item.enabled {
        return;
    }
    let command = item.command;
    close(app);
    app.run_command(command, CmdArgs::None);
}

/// Keys while the menu is open: Up/Down/Home/End move over the enabled items, Enter runs
/// one, Esc closes. Any other key closes the menu and is handled as usual (false), so an
/// explorer key pressed instead of choosing still does its job.
pub fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    let Some(menu) = &mut app.explorer.menu else {
        return false;
    };
    if !key.modifiers.is_empty() {
        close(app);
        return false;
    }
    app.dirty = true;
    match key.code {
        KeyCode::Up => menu.step(-1),
        KeyCode::Down => menu.step(1),
        KeyCode::Home => {
            menu.selected = menu.items.iter().position(|i| i.enabled).unwrap_or(0);
        }
        KeyCode::End => {
            menu.selected = menu.items.iter().rposition(|i| i.enabled).unwrap_or(0);
        }
        // A held Enter never chooses.
        KeyCode::Enter if key.kind == KeyEventKind::Repeat => {}
        KeyCode::Enter => {
            let i = menu.selected;
            choose(app, i);
        }
        KeyCode::Esc => close(app),
        _ => {
            close(app);
            return false;
        }
    }
    true
}

/// Mouse while the menu is open; true when the event is used up. Pointer motion highlights
/// an enabled item, a left press runs one (a dimmed one does nothing). A press outside
/// closes the menu and does nothing else, except a right press in the explorer, which goes
/// on to open the menu there (false).
pub fn handle_mouse(app: &mut App, ev: &MouseEvent) -> bool {
    let Some(menu) = &mut app.explorer.menu else {
        return false;
    };
    let on = menu.item_at(ev.column, ev.row);
    match ev.kind {
        MouseEventKind::Moved => {
            if let Some(i) = on
                && menu.items[i].enabled
                && menu.selected != i
            {
                menu.selected = i;
                app.dirty = true;
            }
        }
        MouseEventKind::Down(button) if !menu.contains(ev.column, ev.row) => {
            close(app);
            let a = app.explorer.area;
            let in_explorer = ev.column >= a.x
                && ev.column < a.x + a.width
                && ev.row >= a.y
                && ev.row < a.y + a.height;
            return !(button == MouseButton::Right && in_explorer);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(i) = on {
                choose(app, i);
            }
        }
        _ => {}
    }
    true
}
