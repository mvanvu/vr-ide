use std::sync::OnceLock;

use cosmic_text::{FontSystem, fontdb};
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::buffer::Cell;
use ratatui::style::{Color, Modifier};
use ratatui::widgets::Paragraph;

use super::*;

/// Loading the system fonts is slow; the tests share one database.
fn font_system() -> FontSystem {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    let db = DB.get_or_init(crate::font::system_database);
    FontSystem::new_with_locale_and_db("en-US".into(), db.clone())
}

fn fonts(config: &FontConfig) -> FontSet {
    let (set, err) = FontSet::with_font_system(font_system(), config);
    assert_eq!(err, None);
    set
}

/// The metrics tests measure a 12 pt font (16 px at scale 1).
const TWELVE_PT: FontConfig = FontConfig {
    family: None,
    size_pt: 12.0,
    line_height: 1.2,
    scale: 1.0,
};

const BG: Rgb = 0x10_2030;
const RED: Rgb = 0xff_0000;

fn palette() -> Palette {
    Palette {
        background: BG,
        foreground: 0xf0_f0f0,
        ..Palette::default()
    }
}

fn backend(width: u32, height: u32) -> PixelBackend {
    PixelBackend::new(fonts(&TWELVE_PT), palette(), width, height)
}

fn cell(symbol: &str, fg: Color, bg: Color) -> Cell {
    let mut cell = Cell::EMPTY;
    cell.set_symbol(symbol).set_fg(fg).set_bg(bg);
    cell
}

/// Pixels that differ between two canvases, as (x, y).
fn changed(a: &Canvas, b: &Canvas) -> Vec<(u32, u32)> {
    let w = a.width();
    a.pixels()
        .iter()
        .zip(b.pixels())
        .enumerate()
        .filter(|(_, (p, q))| p != q)
        .map(|(i, _)| (i as u32 % w, i as u32 / w))
        .collect()
}

/// Pixels of the canvas that are not `color`.
fn not_color(canvas: &Canvas, color: Rgb) -> Vec<(u32, u32)> {
    let blank = Canvas::new(canvas.width(), canvas.height(), color);
    changed(canvas, &blank)
}

#[test]
fn grid_size_comes_from_the_pixel_size_and_the_cell_size() {
    let mut b = backend(1000, 700);
    let m = b.layout().cell;
    assert!(m.width >= 5 && m.height > m.width, "{m:?}");
    // 12 pt at 96 dpi = 16 px, line height 1.2.
    assert_eq!(m.height, 19);
    let size = b.size().unwrap();
    assert_eq!(u32::from(size.width), 1000 / m.width);
    assert_eq!(u32::from(size.height), 700 / m.height);
    let ws = b.window_size().unwrap();
    assert_eq!(ws.columns_rows, size);
    assert_eq!((ws.pixels.width, ws.pixels.height), (1000, 700));
}

#[test]
fn padding_moves_the_grid_and_leftover_pixels_keep_the_background() {
    let mut b = backend(400, 300);
    b.set_padding(4);
    let l = b.layout();
    assert_eq!((l.origin_x, l.origin_y), (4, 4));
    assert_eq!(u32::from(l.cols), (400 - 8) / l.cell.width);
    b.draw([(0, 0, &cell(" ", Color::Reset, Color::Rgb(255, 0, 0)))].into_iter())
        .unwrap();
    assert_eq!(b.canvas().pixel(3, 3), BG);
    assert_eq!(b.canvas().pixel(4, 4), RED);
    assert_eq!(l.cell_at(3, 3), None);
    assert_eq!(l.cell_at(4 + l.cell.width, 4), Some((1, 0)));
}

#[test]
fn a_cell_with_a_known_colour_sets_pixels_inside_that_cell_only() {
    let mut b = backend(300, 200);
    let rect = b.layout().cell_rect(2, 1, 1);
    b.draw([(2, 1, &cell(" ", Color::Reset, Color::Rgb(255, 0, 0)))].into_iter())
        .unwrap();
    let off = not_color(b.canvas(), BG);
    assert_eq!(off.len() as u32, rect.width * rect.height);
    assert!(off.iter().all(|&(x, y)| rect.contains(x, y)));
    assert!(off.iter().all(|&(x, y)| b.canvas().pixel(x, y) == RED));

    // A glyph is drawn in the foreground colour inside the same cell.
    b.draw(
        [(
            2,
            1,
            &cell("M", Color::Rgb(0, 255, 0), Color::Rgb(255, 0, 0)),
        )]
        .into_iter(),
    )
    .unwrap();
    let off = not_color(b.canvas(), BG);
    assert!(off.iter().all(|&(x, y)| rect.contains(x, y)));
    assert!(
        off.iter()
            .any(|&(x, y)| b.canvas().pixel(x, y) == 0x00_ff00)
    );
}

#[test]
fn named_and_indexed_colours_use_the_palette() {
    let mut p = palette();
    p.indexed[4] = 0x12_3456;
    p.indexed[200] = 0x65_4321;
    let mut b = PixelBackend::new(fonts(&FontConfig::default()), p, 200, 100);
    b.draw(
        [
            (0, 0, &cell(" ", Color::Reset, Color::Blue)),
            (1, 0, &cell(" ", Color::Reset, Color::Indexed(200))),
        ]
        .into_iter(),
    )
    .unwrap();
    let r0 = b.layout().cell_rect(0, 0, 1);
    let r1 = b.layout().cell_rect(1, 0, 1);
    assert_eq!(b.canvas().pixel(r0.x, r0.y), 0x12_3456);
    assert_eq!(b.canvas().pixel(r1.x, r1.y), 0x65_4321);
}

#[test]
fn a_wide_symbol_covers_two_cells() {
    let mut b = backend(300, 200);
    let two = b.layout().cell_rect(1, 0, 2);
    let second = b.layout().cell_rect(2, 0, 1);
    b.draw(
        [(
            1,
            0,
            &cell("中", Color::Rgb(255, 255, 255), Color::Rgb(255, 0, 0)),
        )]
        .into_iter(),
    )
    .unwrap();
    let off = not_color(b.canvas(), BG);
    assert!(off.iter().all(|&(x, y)| two.contains(x, y)));
    assert_eq!(
        off.len() as u32,
        two.width * two.height,
        "both cells get the background"
    );
    let ink = |c: &Canvas, r: PixelRect| {
        (r.y..r.bottom())
            .flat_map(|y| (r.x..r.right()).map(move |x| (x, y)))
            .filter(|&(x, y)| c.pixel(x, y) != RED)
            .count()
    };
    assert!(
        ink(b.canvas(), second) > 0,
        "the glyph reaches into the second cell"
    );

    // Replacing it by a narrow symbol clears the right half even though ratatui sends one cell.
    b.draw(
        [(
            1,
            0,
            &cell("a", Color::Rgb(255, 255, 255), Color::Rgb(255, 0, 0)),
        )]
        .into_iter(),
    )
    .unwrap();
    assert_eq!(
        ink(b.canvas(), second),
        0,
        "the right half shows only the old background"
    );
}

#[test]
fn combining_sequences_are_one_glyph_in_one_cell() {
    let mut b = backend(300, 200);
    let before = b.fonts().cached_glyphs();
    let rect = b.layout().cell_rect(3, 2, 1);
    b.draw(
        [(
            3,
            2,
            &cell(
                "e\u{0323}\u{0302}",
                Color::Rgb(255, 255, 255),
                Color::Rgb(255, 0, 0),
            ),
        )]
        .into_iter(),
    )
    .unwrap();
    assert_eq!(b.fonts().cached_glyphs(), before + 1);
    let off = not_color(b.canvas(), BG);
    assert!(off.iter().all(|&(x, y)| rect.contains(x, y)));
    let ink = off
        .iter()
        .filter(|&&(x, y)| b.canvas().pixel(x, y) != RED)
        .count();
    assert!(ink > 10, "the letter and its marks are drawn");

    // Precomposed Vietnamese letters with stacked marks stay in their cell too.
    for (col, s) in [(5, "ệ"), (6, "ở"), (7, "Ặ")] {
        let rect = b.layout().cell_rect(col, 2, 1);
        let before = b.canvas().clone();
        b.draw([(col, 2, &cell(s, Color::Rgb(255, 255, 255), Color::Reset))].into_iter())
            .unwrap();
        assert!(
            changed(&before, b.canvas())
                .iter()
                .all(|&(x, y)| rect.contains(x, y)),
            "{s}"
        );
    }
}

#[test]
fn emoji_stay_inside_their_two_cells() {
    let mut b = backend(300, 200);
    let two = b.layout().cell_rect(0, 0, 2);
    b.draw([(0, 0, &cell("👍", Color::Reset, Color::Reset))].into_iter())
        .unwrap();
    assert!(
        not_color(b.canvas(), BG)
            .iter()
            .all(|&(x, y)| two.contains(x, y))
    );
}

#[test]
fn drawing_changed_cells_leaves_other_pixels_untouched() {
    let mut b = backend(400, 200);
    let size = b.size().unwrap();
    let cells: Vec<_> = (0..size.height)
        .flat_map(|y| (0..size.width).map(move |x| (x, y)))
        .map(|(x, y)| {
            (
                x,
                y,
                cell(
                    if (x + y).is_multiple_of(2) {
                        "x"
                    } else {
                        "ở"
                    },
                    Color::Rgb(200, 200, 0),
                    Color::Rgb(0, 0, (x * 3) as u8),
                ),
            )
        })
        .collect();
    b.draw(cells.iter().map(|(x, y, c)| (*x, *y, c))).unwrap();
    b.take_damage();
    let before = b.canvas().clone();
    let rect = b.layout().cell_rect(4, 3, 1);
    b.draw(
        [(
            4,
            3,
            &cell("Q", Color::Rgb(255, 255, 255), Color::Rgb(255, 0, 0)),
        )]
        .into_iter(),
    )
    .unwrap();
    let diff = changed(&before, b.canvas());
    assert!(!diff.is_empty());
    assert!(diff.iter().all(|&(x, y)| rect.contains(x, y)));
    assert_eq!(b.take_damage(), vec![rect]);
    assert_eq!(b.take_damage(), vec![]);
}

#[test]
fn modifiers_draw_lines_and_swap_colours() {
    let mut b = backend(300, 200);
    let m = b.layout().cell;
    let rect = b.layout().cell_rect(0, 0, 1);
    let mut c = cell(" ", Color::Rgb(0, 255, 0), Color::Rgb(255, 0, 0));
    c.modifier = Modifier::UNDERLINED;
    b.draw([(0, 0, &c)].into_iter()).unwrap();
    let y = rect.y + m.underline as u32;
    assert_eq!(b.canvas().pixel(rect.x, y), 0x00_ff00);
    assert_eq!(b.canvas().pixel(rect.x, rect.y), RED);
    c.modifier = Modifier::REVERSED;
    b.draw([(0, 0, &c)].into_iter()).unwrap();
    assert_eq!(b.canvas().pixel(rect.x, rect.y), 0x00_ff00);
    c.modifier = Modifier::CROSSED_OUT;
    c.bg = Color::Rgb(0, 0, 0);
    b.draw([(0, 0, &c)].into_iter()).unwrap();
    assert_eq!(
        b.canvas().pixel(rect.x, rect.y + m.strike as u32),
        0x00_ff00
    );
}

#[test]
fn cursor_shapes_are_drawn_and_moving_the_cursor_restores_the_cell() {
    let mut b = backend(300, 200);
    let mut p = palette();
    p.cursor = Some(0x00_00ff);
    b.set_palette(p);
    let m = b.layout().cell;
    let red = cell(" ", Color::Reset, Color::Rgb(255, 0, 0));
    b.draw([(1, 1, &red), (2, 1, &red)].into_iter()).unwrap();
    b.hide_cursor().unwrap();
    b.flush().unwrap();
    let plain = b.canvas().clone();
    b.show_cursor().unwrap();
    let rect = b.layout().cell_rect(1, 1, 1);
    b.set_cursor_position((1, 1)).unwrap();

    b.set_cursor_shape(CursorShape::Block);
    b.flush().unwrap();
    assert_eq!(b.canvas().pixel(rect.x, rect.y), 0x00_00ff);
    assert_eq!(
        b.canvas().pixel(rect.right() - 1, rect.bottom() - 1),
        0x00_00ff
    );

    b.set_cursor_shape(CursorShape::Bar);
    b.flush().unwrap();
    assert_eq!(b.canvas().pixel(rect.x, rect.y + 3), 0x00_00ff);
    assert_eq!(b.canvas().pixel(rect.right() - 1, rect.y + 3), RED);

    b.set_cursor_shape(CursorShape::Underline);
    b.flush().unwrap();
    assert_eq!(b.canvas().pixel(rect.x + 1, rect.bottom() - 1), 0x00_00ff);
    assert_eq!(b.canvas().pixel(rect.x + 1, rect.y), RED);
    assert!(
        changed(&plain, b.canvas())
            .iter()
            .all(|&(x, y)| rect.contains(x, y))
    );
    assert_eq!(
        2 * m.line,
        (rect.y..rect.bottom())
            .filter(|&y| b.canvas().pixel(rect.x + 1, y) == 0x00_00ff)
            .count() as u32
    );

    b.set_cursor_position((2, 1)).unwrap();
    b.flush().unwrap();
    let next = b.layout().cell_rect(2, 1, 1);
    assert!(
        changed(&plain, b.canvas())
            .iter()
            .all(|&(x, y)| next.contains(x, y)),
        "old cell restored"
    );

    b.hide_cursor().unwrap();
    b.flush().unwrap();
    assert_eq!(changed(&plain, b.canvas()), vec![]);
    b.show_cursor().unwrap();
    b.set_cursor_blink_on(false);
    b.flush().unwrap();
    assert_eq!(changed(&plain, b.canvas()), vec![]);
}

#[test]
fn a_redrawn_cell_under_the_cursor_gets_the_cursor_back() {
    let mut b = backend(300, 200);
    b.set_cursor_position((0, 0)).unwrap();
    b.flush().unwrap();
    let with_cursor = b.canvas().clone();
    b.draw([(0, 0, &Cell::EMPTY)].into_iter()).unwrap();
    b.flush().unwrap();
    assert_eq!(changed(&with_cursor, b.canvas()), vec![]);
}

#[test]
fn preedit_is_drawn_underlined_and_removed_cleanly() {
    let mut b = backend(400, 200);
    b.hide_cursor().unwrap();
    b.flush().unwrap();
    let plain = b.canvas().clone();
    b.set_preedit(Some(Preedit {
        text: "việt中".into(),
        col: 2,
        row: 1,
        caret: None,
    }));
    b.flush().unwrap();
    // 4 narrow cells + one wide = 6 cells from col 2.
    let area = b.layout().cell_rect(2, 1, 6);
    let diff = changed(&plain, b.canvas());
    assert!(!diff.is_empty());
    assert!(diff.iter().all(|&(x, y)| area.contains(x, y)));
    let underline_y = area.y + b.layout().cell.underline as u32;
    assert_eq!(b.canvas().pixel(area.x, underline_y), palette().foreground);
    b.set_preedit(None);
    b.flush().unwrap();
    assert_eq!(changed(&plain, b.canvas()), vec![]);
}

#[test]
fn resize_relays_the_grid_and_scale_grows_the_cells() {
    let mut b = backend(300, 200);
    let small = b.layout();
    b.take_damage();
    b.resize(600, 400, 1.0).unwrap();
    assert_eq!(b.canvas().width(), 600);
    assert_eq!(u32::from(b.size().unwrap().width), 600 / small.cell.width);
    assert_eq!(b.take_damage(), vec![PixelRect::new(0, 0, 600, 400)]);
    b.resize(600, 400, 2.0).unwrap();
    let big = b.layout();
    assert_eq!(big.cell.height, 38);
    assert!(big.cell.width >= 2 * small.cell.width - 1);
    assert_eq!(u32::from(big.cols), 600 / big.cell.width);
    // A cell drawn after the change lands in the new grid.
    b.draw([(1, 1, &cell(" ", Color::Reset, Color::Rgb(255, 0, 0)))].into_iter())
        .unwrap();
    let rect = big.cell_rect(1, 1, 1);
    assert_eq!(b.canvas().pixel(rect.x, rect.y), RED);
    assert_eq!(b.canvas().pixel(rect.x - 1, rect.y), BG);
    assert!(matches!(
        b.resize(600, 400, f32::NAN),
        Err(FontError::InvalidScale(_))
    ));
    assert_eq!(b.layout(), big, "a bad scale keeps the old one");
}

#[test]
fn a_missing_font_family_is_an_error_and_the_default_is_used() {
    let config = FontConfig {
        family: Some("No Such Font Family 1234".into()),
        ..FontConfig::default()
    };
    let (set, err) = FontSet::with_font_system(font_system(), &config);
    assert_eq!(
        err,
        Some(FontError::FamilyNotFound("No Such Font Family 1234".into()))
    );
    let (default, _) = FontSet::with_font_system(font_system(), &FontConfig::default());
    assert_eq!(set.family(), default.family());
    assert!(!set.family().is_empty());
    assert_eq!(set.metrics(), default.metrics());

    // A reload with a bad family keeps the working font.
    let mut set = set;
    assert!(set.set_config(&config).is_err());
    assert_eq!(set.family(), default.family());
    assert_eq!(
        set.set_config(&FontConfig {
            size_pt: -1.0,
            ..FontConfig::default()
        }),
        Err(FontError::InvalidSize(-1.0))
    );
}

#[test]
fn a_named_family_resolves_case_insensitively() {
    let (default, _) = FontSet::with_font_system(font_system(), &FontConfig::default());
    let name = default.family().to_uppercase();
    let (set, err) = FontSet::with_font_system(
        font_system(),
        &FontConfig {
            family: Some(name),
            ..FontConfig::default()
        },
    );
    assert_eq!(err, None);
    assert_eq!(set.family(), default.family());
}

#[test]
fn ratatui_terminal_draws_through_the_backend() {
    let b = backend(400, 200);
    let mut term = Terminal::new(b).unwrap();
    term.draw(|f| {
        f.render_widget(
            Paragraph::new("xin chào")
                .style(ratatui::style::Style::new().bg(Color::Rgb(255, 0, 0))),
            f.area(),
        );
        f.set_cursor_position((3, 0));
    })
    .unwrap();
    let b = term.backend_mut();
    assert_eq!(b.get_cursor_position().unwrap(), Position::new(3, 0));
    let rect = b.layout().cell_rect(0, 0, 1);
    assert_eq!(b.canvas().pixel(rect.x, rect.y), RED);
    assert!(!b.take_damage().is_empty());
}

/// A backend whose canvas leaves `slack` spare pixels right and below a cols x rows grid.
fn backend_with_slack(cols: u32, rows: u32, slack: u32) -> PixelBackend {
    let m = fonts(&TWELVE_PT).metrics();
    backend(cols * m.width + slack, rows * m.height + slack)
}

fn red_at(b: &PixelBackend, col: u16, row: u16) -> bool {
    let r = b.layout().cell_rect(col, row, 1);
    (r.y..r.bottom()).all(|y| (r.x..r.right()).all(|x| b.canvas().pixel(x, y) == RED))
}

#[test]
fn relayout_that_keeps_the_grid_size_keeps_the_cells() {
    let mut b = backend_with_slack(10, 5, 7);
    let red = cell(" ", Color::Reset, Color::Rgb(255, 0, 0));
    b.draw([(2, 1, &red), (9, 4, &red)].into_iter()).unwrap();
    let grid = b.size().unwrap();

    // Same size, same scale (a spurious resize event): ratatui sees no size change, so it
    // will not redraw; the cells must survive.
    let (w, h) = (b.canvas().width(), b.canvas().height());
    b.resize(w, h, 1.0).unwrap();
    assert_eq!(b.size().unwrap(), grid);
    assert!(red_at(&b, 2, 1) && red_at(&b, 9, 4), "same-size resize");

    // A slightly bigger window with the same number of cells.
    b.resize(w + 2, h + 2, 1.0).unwrap();
    assert_eq!(b.size().unwrap(), grid);
    assert!(red_at(&b, 2, 1) && red_at(&b, 9, 4), "same-grid resize");

    // Padding that still fits the same grid moves the cells.
    b.set_padding(3);
    assert_eq!(b.size().unwrap(), grid);
    assert_eq!(b.layout().origin_x, 3);
    assert!(red_at(&b, 2, 1) && red_at(&b, 9, 4), "padding change");
    assert_eq!(b.canvas().pixel(0, 0), BG);

    // A font reload with the same metrics.
    let family = b.fonts().family().to_uppercase();
    b.set_font_config(&FontConfig {
        family: Some(family),
        ..TWELVE_PT
    })
    .unwrap();
    assert_eq!(b.size().unwrap(), grid);
    assert!(red_at(&b, 2, 1) && red_at(&b, 9, 4), "font change");
    assert_eq!(b.take_damage(), vec![b.canvas().bounds()]);
}

#[test]
fn relayout_through_ratatui_keeps_the_screen() {
    let b = backend_with_slack(10, 5, 7);
    let mut term = Terminal::new(b).unwrap();
    let ui = |f: &mut ratatui::Frame| {
        f.render_widget(
            Paragraph::new("x").style(ratatui::style::Style::new().bg(Color::Rgb(255, 0, 0))),
            f.area(),
        );
    };
    term.draw(ui).unwrap();
    let (w, h) = (
        term.backend().canvas().width(),
        term.backend().canvas().height(),
    );
    term.backend_mut().resize(w + 1, h + 1, 1.0).unwrap();
    term.draw(ui).unwrap();
    assert!(red_at(term.backend(), 5, 2) && red_at(term.backend(), 9, 4));
}

fn ink_free(b: &PixelBackend, col: u16, row: u16, bg: Rgb) -> bool {
    let r = b.layout().cell_rect(col, row, 1);
    (r.y..r.bottom()).all(|y| (r.x..r.right()).all(|x| b.canvas().pixel(x, y) == bg))
}

fn underlined(b: &PixelBackend, col: u16, row: u16, color: Rgb) -> bool {
    let r = b.layout().cell_rect(col, row, 1);
    let y = r.y + b.layout().cell.underline as u32;
    (r.x..r.right()).all(|x| b.canvas().pixel(x, y) == color)
}

/// A row with a wide red cell at cols 4-5 and the cursor hidden.
fn wide_row() -> (PixelBackend, Canvas) {
    let mut b = backend(400, 200);
    b.hide_cursor().unwrap();
    b.draw(
        [(
            4,
            1,
            &cell("中", Color::Rgb(255, 255, 255), Color::Rgb(255, 0, 0)),
        )]
        .into_iter(),
    )
    .unwrap();
    b.flush().unwrap();
    let plain = b.canvas().clone();
    (b, plain)
}

#[test]
fn preedit_ending_on_a_wide_cell_keeps_its_last_grapheme_and_no_stale_half() {
    let (mut b, plain) = wide_row();
    let fg = palette().foreground;
    // "ab" from col 3: 'b' lands on the wide cell's left half; col 5 was its right half.
    b.set_preedit(Some(Preedit {
        text: "ab".into(),
        col: 3,
        row: 1,
        caret: None,
    }));
    b.flush().unwrap();
    assert!(underlined(&b, 3, 1, fg), "first grapheme underlined");
    assert!(
        underlined(&b, 4, 1, 0xff_ffff),
        "last grapheme still shown, underlined"
    );
    assert!(
        ink_free(&b, 5, 1, RED),
        "no right half of the covered wide glyph"
    );
    b.set_preedit(None);
    b.flush().unwrap();
    assert_eq!(changed(&plain, b.canvas()), vec![], "wide cell restored");
}

#[test]
fn preedit_starting_on_a_wide_cells_right_half_leaves_no_left_half() {
    let (mut b, plain) = wide_row();
    b.set_preedit(Some(Preedit {
        text: "xy".into(),
        col: 5,
        row: 1,
        caret: None,
    }));
    b.flush().unwrap();
    assert!(
        ink_free(&b, 4, 1, RED),
        "no left half of the covered wide glyph"
    );
    assert!(underlined(&b, 5, 1, palette().foreground));
    assert!(underlined(&b, 6, 1, palette().foreground));
    b.set_preedit(None);
    b.flush().unwrap();
    assert_eq!(changed(&plain, b.canvas()), vec![]);
}

#[test]
fn preedit_across_a_wide_cell_mid_text_is_fully_drawn() {
    let (mut b, plain) = wide_row();
    // 'b' over the lead, 'c' over the right half, 'd' after it.
    b.set_preedit(Some(Preedit {
        text: "abcd".into(),
        col: 3,
        row: 1,
        caret: None,
    }));
    b.set_cursor_position((7, 1)).unwrap();
    b.flush().unwrap();
    assert!(underlined(&b, 4, 1, 0xff_ffff));
    assert!(underlined(&b, 5, 1, palette().foreground));
    assert!(underlined(&b, 6, 1, palette().foreground));
    b.set_preedit(None);
    b.flush().unwrap();
    assert_eq!(changed(&plain, b.canvas()), vec![]);
}

#[test]
fn huge_padding_does_not_overflow() {
    let mut b = backend(300, 200);
    b.set_padding(u32::MAX);
    assert_eq!(b.size().unwrap(), Size::new(1, 1));
    b.draw([(0, 0, &cell("x", Color::Reset, Color::Reset))].into_iter())
        .unwrap();
    b.resize(300, 200, 16.0).unwrap();
    assert_eq!(b.size().unwrap(), Size::new(1, 1));
}

/// FR-1 U2: at start, a family that is not installed keeps the asked size and line height (as
/// a reload does); only an invalid size or line height gives the whole default, whose size is
/// the settings' default (11 pt).
#[test]
fn a_missing_family_at_start_keeps_size_and_line_height() {
    let config = FontConfig {
        family: Some("No Such Font Family 1234".into()),
        size_pt: 16.0,
        line_height: 1.5,
        scale: 1.0,
    };
    let (set, err) = FontSet::with_font_system(font_system(), &config);
    assert!(matches!(err, Some(FontError::FamilyNotFound(_))), "{err:?}");
    assert_eq!(set.config().family, None);
    assert_eq!(
        (set.config().size_pt, set.config().line_height),
        (16.0, 1.5)
    );
    let asked = fonts(&FontConfig {
        family: None,
        ..config.clone()
    });
    assert_eq!(set.metrics(), asked.metrics());

    let (set, err) = FontSet::with_font_system(
        font_system(),
        &FontConfig {
            size_pt: 0.0,
            ..config
        },
    );
    assert_eq!(err, Some(FontError::InvalidSize(0.0)));
    assert_eq!(set.config(), &FontConfig::default());
    assert_eq!(FontConfig::default().size_pt, 11.0);
}

/// FR-1 U4: the pixels beyond the last whole cell take the colour of the last row's and last
/// column's cells (a status bar reaches the window's bottom edge), and are presented with them.
#[test]
fn leftover_pixels_take_the_edge_cells_background() {
    const GREEN: Rgb = 0x00_ff00;
    let (w, h) = (301, 203);
    let mut b = backend(w, h);
    let l = b.layout();
    assert!(u32::from(l.cols) * l.cell.width < w, "{l:?}");
    assert!(u32::from(l.rows) * l.cell.height < h, "{l:?}");
    let (last_col, last_row) = (l.cols - 1, l.rows - 1);
    b.take_damage();
    let status = cell(" ", Color::Reset, Color::Rgb(255, 0, 0));
    let side = cell(" ", Color::Reset, Color::Rgb(0, 255, 0));
    let mut cells: Vec<(u16, u16, &Cell)> = (0..l.cols).map(|c| (c, last_row, &status)).collect();
    cells.push((last_col, 0, &side));
    b.draw(cells.into_iter()).unwrap();
    let c = b.canvas();
    assert_eq!(c.pixel(w - 1, h - 1), RED, "bottom-right corner");
    assert_eq!(c.pixel(0, h - 1), RED, "under the status bar");
    assert_eq!(c.pixel(w - 1, 0), GREEN, "right of the last column");
    assert_eq!(
        c.pixel(w - 1, l.cell.height),
        BG,
        "a row not drawn keeps the background"
    );
    let damage = b.take_damage();
    for (x, y) in [(w - 1, h - 1), (0, h - 1), (w - 1, 0)] {
        assert!(
            damage.iter().any(|r| r.contains(x, y)),
            "({x}, {y}) in {damage:?}"
        );
    }

    // A block cursor on the last cell does not paint the leftover in the cursor colour.
    let mut p = palette();
    p.cursor = Some(0x00_00ff);
    b.set_palette(p);
    b.set_cursor_shape(CursorShape::Block);
    b.set_cursor_position((last_col, last_row)).unwrap();
    b.flush().unwrap();
    assert_eq!(b.canvas().pixel(w - 1, h - 1), RED);
}

/// FR-1 U5: while composing, the caret is drawn where the IME puts it inside the preedit, and
/// not at all when the IME hides it.
#[test]
fn the_caret_follows_the_ime_cursor_inside_the_preedit() {
    let mut b = backend(400, 200);
    let mut p = palette();
    p.cursor = Some(0x00_00ff);
    b.set_palette(p);
    b.set_cursor_shape(CursorShape::Block);
    b.set_cursor_position((2, 1)).unwrap();
    b.set_preedit(Some(Preedit {
        text: "việt".into(),
        col: 2,
        row: 1,
        caret: Some(3),
    }));
    b.flush().unwrap();
    let at = |b: &PixelBackend, col: u16| {
        let r = b.layout().cell_rect(col, 1, 1);
        b.canvas().pixel(r.x, r.y)
    };
    assert_eq!(at(&b, 5), 0x00_00ff, "caret after the third column");
    assert_ne!(at(&b, 2), 0x00_00ff, "not at the preedit start");

    b.set_preedit(Some(Preedit {
        text: "việt".into(),
        col: 2,
        row: 1,
        caret: None,
    }));
    b.flush().unwrap();
    assert_eq!(
        not_color(b.canvas(), 0x00_00ff).len() as u32,
        400 * 200,
        "no caret"
    );
}

/// FR-1 U6: an unfocused window shows the block cursor as an outline.
#[test]
fn a_hollow_block_cursor_is_an_outline() {
    let mut b = backend(300, 200);
    let mut p = palette();
    p.cursor = Some(0x00_00ff);
    b.set_palette(p);
    let red = cell(" ", Color::Reset, Color::Rgb(255, 0, 0));
    b.draw([(1, 1, &red)].into_iter()).unwrap();
    b.set_cursor_position((1, 1)).unwrap();
    b.set_cursor_shape(CursorShape::HollowBlock);
    b.flush().unwrap();
    let r = b.layout().cell_rect(1, 1, 1);
    let px = |x, y| b.canvas().pixel(x, y);
    for (x, y) in [
        (r.x, r.y),
        (r.right() - 1, r.y),
        (r.x, r.bottom() - 1),
        (r.right() - 1, r.bottom() - 1),
    ] {
        assert_eq!(px(x, y), 0x00_00ff, "outline at ({x}, {y})");
    }
    assert_eq!(
        px(r.x + r.width / 2, r.y + r.height / 2),
        RED,
        "inside stays"
    );
}
